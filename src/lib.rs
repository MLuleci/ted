extern crate termion;
extern crate getopts;
extern crate unicode_segmentation;
extern crate unicode_width;

pub mod buffer;
pub mod screen;

use crate::screen::Screen;
use crate::screen::cursor::Direction;
use screen::Message;
use termion::event::{Key, Event, MouseEvent};
use termion::input::{TermRead, MouseTerminal};
use std::cmp::min;
use std::io::{stdin, stdout, ErrorKind, Write};
use termion::raw::IntoRawMode;
use std::error::Error;
use getopts::Options;
use std::process;

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options] [file ...]", program);
    println!("{}", opts.usage(&brief));
}

#[derive(Debug)]
pub struct Config {
    paths: Vec<String>,
    readonly: bool,
    truncate: bool
}

impl Config {
    pub fn build(args: &[String]) -> Result<Config, String> {
        let mut opts = Options::new();
        opts.optflag("t", "truncate", "Truncate existing file(s)");
        opts.optflag("r", "readonly", "Open file(s) as read-only");
        opts.optflag("h", "help", "Print this help menu");

        let program = &args[0];
        let matches = opts.parse(&args[1..]);

        if let Err(f) = matches {
            return Err(f.to_string());
        }
        let matches = matches.unwrap();

        if matches.opt_present("h") {
            print_usage(program, opts);
            process::exit(1);
        }

        let readonly = matches.opt_present("r");
        let truncate = matches.opt_present("t");

        if readonly && truncate {
            return Err("Cannot truncate files in read-only mode".to_string());
        }
        
        Ok(Config { 
            paths: matches.free,
            readonly,
            truncate
        })
    }
}

pub fn run(config: Config) -> Result<(), Box<dyn Error>> {
    let mut screens: Vec<Screen> = config.paths
        .iter()
        .map(|p| Screen::new(p, &config))
        .collect();

    if config.paths.is_empty() {
        screens.push(Screen::new("", &config));
    }

    let stdin = stdin();
    let mut stdout = MouseTerminal::from(stdout().into_raw_mode().unwrap());
    let mut index = 0;
    let mut chord = false;
    let mut timeout = 0;

    let mut events = stdin.events();
    loop {
        let screen = &mut screens[index];

        if timeout == 0 {
            screen.clear_message();
        } 
        
        if timeout >= 0 {
            timeout -= 1;
        }

        screen.draw(&mut stdout)?;
        stdout.flush()?;

        if let Some(event) = events.next() {
            if chord {
                chord = false;
                timeout = 0;
                let mut was_valid = true;

                match event? {
                    Event::Key(Key::Esc) => continue,
                    Event::Key(Key::Char(ch)) => {
                        match ch {
                            'q' => break,
                            'z' => screen.undo(),
                            'y' => screen.redo(),
                            '.' => index = (index + 1) % screens.len(),
                            'n' => {
                                screens.push(Screen::new("", &config));
                                index = screens.len() - 1;
                            },
                            ',' => {
                                if index == 0 {
                                    index = screens.len() - 1;
                                } else {
                                    index -= 1;
                                }
                            },
                            'o' => {
                                if let Some(reply) = screen.prompt(&mut events, &mut stdout, "Open file:")? {
                                    screens.push(Screen::new(&reply, &config));
                                    index = screens.len() - 1;
                                }
                            },
                            'w' | 's' => {
                                let should_save = 
                                    screen.is_dirty() && (
                                        ch == 's'
                                        || screen.confirm_prompt(
                                            &mut events, 
                                            &mut stdout, 
                                            "Save changes (Y/n)", 
                                            true
                                        )?
                                    );

                                if should_save {
                                    // Try normally first...
                                    if let Err(e) = screen.write(false) {
                                        // ...if it fails...
                                        match e.kind() {
                                            ErrorKind::AlreadyExists => {
                                                // ...ask user if they want to overwrite
                                                let overwrite = screen.confirm_prompt(
                                                    &mut events, 
                                                    &mut stdout,
                                                    "Overwrite (y/N)?",
                                                    false
                                                )?;

                                                if overwrite {
                                                    if let Err(e) = screen.write(true) {
                                                        // don't crash if we still can't write save
                                                        screen.set_message(Message::Error(e.to_string()));
                                                        timeout = 5;
                                                        continue;
                                                    }
                                                }
                                            },
                                            _ => {
                                                // ...show error and stop
                                                screen.set_message(Message::Error(e.to_string()));
                                                timeout = 5;
                                                continue;
                                            }
                                        }
                                    }
                                }

                                if ch == 'w' {
                                    screens.remove(index);
                                    if screens.is_empty() { 
                                        break;
                                    } else {
                                        index = min(screens.len() - 1, index);
                                    }
                                }
                            },
                            'p' => {
                                if let Some(reply) = screen.prompt(&mut events, &mut stdout, "Switch to buffer:")? {
                                    // Look for a buffer whose file name includes `reply` somewhere:
                                    let found = screens
                                        .iter()
                                        .enumerate()
                                        .find(|(_, s)| {
                                            s.path()
                                                .file_name()
                                                .and_then(|o| o.to_str())
                                                .map_or(
                                                    false, 
                                                    |n| n.starts_with(&reply)
                                                )
                                        })
                                        .map(|i| i.0);

                                    if let Some(i) = found {
                                        index = i;
                                    } else {
                                        let m = format!("Buffer '{reply}' not found");
                                        screens[index].set_message(Message::Warning(m));
                                        timeout = 3;
                                    }
                                }
                            },
                            _ => was_valid = false
                        }
                    }
                    Event::Key(Key::Up) => screen.top(),
                    Event::Key(Key::Down) => screen.bottom(),
                    _ => was_valid = false
                }

                if !was_valid {
                    screens[index].set_message(Message::Warning(String::from("Unknown chord")));
                    timeout = 3;
                }
            } else {
                match event? {
                    Event::Key(Key::Char(ch)) => {
                        if screen.overwrite {
                            screen.overwrite(ch);
                        } else {
                            screen.insert(ch)
                        }
                    },
                    Event::Key(Key::Insert) => {
                        screen.overwrite = !screen.overwrite;
                    },
                    Event::Key(Key::Ctrl(ch)) => {
                        if ch == 'x' && !chord {
                            chord = true;
                            timeout = -1;
                            let m = String::from("Waiting for C-x chord (Esc to cancel)");
                            screen.set_message(Message::Info(m));
                        }
                    },
                    Event::Key(Key::Backspace) => screen.backspace(),
                    Event::Key(Key::Delete) => screen.delete(),
                    Event::Key(Key::Home) => screen.home(),
                    Event::Key(Key::End) => screen.end(),
                    Event::Key(Key::Up) => screen.move_cursor(Direction::Up),
                    Event::Key(Key::Down) => screen.move_cursor(Direction::Down),
                    Event::Key(Key::Left) => screen.move_cursor(Direction::Left),
                    Event::Key(Key::Right) => screen.move_cursor(Direction::Right),
                    Event::Mouse(me) => {
                        match me {
                            MouseEvent::Press(_, x, y) => 
                            screen.set_cursor((x - 1) as usize, (y - 1) as usize),
                            _ => (),
                        }
                    },
                    _ => ()
                }
            }
        }

        assert!(index < screens.len(), "screen index out-of-range");
    }

    write!(stdout, "{}{}{}", termion::clear::All, termion::cursor::Goto(1, 1), termion::cursor::BlinkingBar)?;

    Ok(())
}