# Opening a file:
1. Try opening file (no write)
    1.1. Not found -> Create named default
    1.2. Other error(s) -> Create default (& error)
2. Try reading file or metadata
    2.1. Error -> Create default (& error)

# Classes:
- Config: All relevant configuration (arguments & environment)
- Buffer: All text operations and file I/O
    - Line: Per-line text operations, directed by Buffer
- Screen: All text display (incl. status and I/O errors)
    - Cursor: Point in buffer, directed by Screen

Each Screen owns a Buffer and 1+ Cursor objects.
The main loop sends events to the Screen, and it handles them:
- Modifying text:
    1. Builds a command using Cursor position(s) and event details
    2. Sends command to Buffer, receives back undo command, stores it on a stack
    3. Adjusts Cursor(s) to final position(s)
- Moving the cursor: Calls relevant method on the Cursor(s)

Each Buffer owns a vector of Line objects.
When a command is received:
1. It is divided it into smaller commands to be applied to each relevant Line
2. If necessary, Line(s) are deleted
3. Remaining Line(s) execute the changes and update their internal state
4. Undo command is generated from the initial command and returned

# Notes:
- crossterm uses kitty protocol, which is only implemented by some terminals
- terminals send three types of events:
    - text events: just the bytes for the keys, from 0x20 to 0x7E (ASCII printables)
    - escape codes: C0 codes, which are the ASCII non-printables e.g. ENTER or DEL
    - control sequences: CSI i.e. ESC [ which can contain parameters ending with a command
- pressing a key with the modifier:
    - none: just sends the text bytes or the escape code
    - CTRL: only some keys can be used with CTRL + key, and they correspond to the escape codes you can otherwise send e.g. CTRL + m = ENTER
    - ALT: ESC + key
    - SHIFT: Upper case of the key
- The reason why CTRL + tab and tab both send 0x9 is because tab is already a C0 escape code, so the CTRL modifier doesn't change it. A SHIFT + tab however sends a CSI Z (i.e. ^Z or CTRL + Z) sequence

In conclusion, the legacy terminal protocol sucks!!1!one

Links:
- kitty: https://sw.kovidgoyal.net/kitty/keyboard-protoco
- VT100 reference: https://vt100.net/docs/vt100-ug/chapter3.html

# TODOs:
- Clear messages at appropriate times
- Shortcuts:
    - M-s: save (done)
    - M->: switch buffer (done)
    - M-<: switch back buffer (done)
    - C-Left/Right: move and select
    - C-Up/Down: goto start or end of buffer respectively
    - M-c: copy
    - M-x: cut
    - M-p: paste
    - M-z: undo (done)
    - M-Z: redo (done)
    - M-n: new buffer (done)
    - M-o: open by path (done)
    - M-w: close buffer (done)
    - M-f: find
    - M-h: find and replace
    - M-p: switch to buffer by name (done)
    - M-j: jump to line