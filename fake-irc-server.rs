// The MIT License (MIT)
//
// Copyright (c) 2019 Adit Cahya Ramadhan
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

//! Fake IRC server for testing a plugin on WeeChat

use std::io::{Write, BufRead};
use std::io::BufReader;
use std::io;
use std::net::{TcpStream, TcpListener};
use std::iter::Peekable;
use std::str::CharIndices;
use std::thread;
use std::sync::mpsc::{self, Sender, Receiver, TryRecvError};


macro_rules! try_expect {
    ($result:expr, $reason:literal) => {
        match $result {
            Ok(val) => val,
            Err(_) => {
                eprintln!($reason);
                return;
            }
        }
    };
}


macro_rules! debug {
    ($($tt:tt)*) => { eprintln!($($tt)*) };
}


macro_rules! send_message {
    ($writer:expr, $msg:literal, $($tt:tt)*) => {
        debug!(concat!("=== Sending: ", $msg), $($tt)*);
        try_expect!(
            write!($writer, concat!($msg, "\r\n"), $($tt)*),
            "Error writing to TcpStream"
        );
    };
}


const SERVER: &str = "127.0.0.1";
const PROGRAMVER: &str = "fake-irc-server-v0.1.0";


fn main() {
    let mut args = std::env::args();
    // ignore program name
    args.next();
    let port = match args.next() {
        Some(s) => try_expect!(s.parse(), "PORT argument is not a number. Usage: fake-irc-server [PORT]"),
        None => 1234, // default port
    };

    let serverport = format!("{server}:{port}", server=SERVER, port=port);
    debug!("=== Listening on {}", serverport);
    let listener = try_expect!(
        TcpListener::bind(&serverport),
        "Can't create TcpListener"
    );

    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || process_stdin(receiver));

    // This will loop forever
    for stream in listener.incoming() {
        let sender = sender.clone();
        let stream = try_expect!(stream, "Error on incoming stream");
        thread::spawn(move || process_stream(stream, port, sender));
    }
}


/// Process any incoming IRC connection. Automatically reply any PING message with a PONG.
/// Process registratioin handshake by sending the 001 002 003 004 and 005 numeric reply
/// to the client.
fn process_stream(stream: TcpStream, port: usize, sender: Sender<TcpStream>) {
    debug!("=== Getting new incoming connection");

    let mut buff = String::new();
    let mut reader = BufReader::new(stream);

    // Processing 1: NICK <nick>
    // Processing 2: USER <user> 0 * :<real>

    let mut nick = None;
    let mut user = None;
    let mut real = None;

    let mut registration_finished = false;

    loop {
        buff.clear();
        match reader.read_line(&mut buff) {
            Ok(0) => return, // EOF
            Ok(_) => {
                // remove \r\n from the buff
                match buff.pop() {
                    Some('\n') => (),
                    _ => continue, // last character isn't \n
                }
                match buff.pop() {
                    Some('\r') => (),
                    _ => continue, // second to last character isn't \r
                }
            },
            Err(_) => {
                eprintln!("Error reading from TcpStream");
                return;
            },
        }

        let message = match IrcMessage::new(&buff) {
            Ok(m) => m,
            Err(_) => continue, // ignore message error; for now
        };

        match &message {
            IrcMessage { command, params, .. } if upcase_eq(&command, "NICK") => {
                nick = params.get(0).cloned();
                debug!("=== Message: {:?}", message);
            },
            IrcMessage { command, params, .. } if upcase_eq(&command, "USER") => {
                user = params.get(0).cloned();
                real = params.get(3).cloned();
                debug!("=== Message: {:?}", message);
            },
            IrcMessage { command, params, .. } if upcase_eq(&command, "PING") => {
                let param = params.get(0).cloned().unwrap_or_default();

                // Not using the send_message! macro since we don't want it to be
                // logged. The CRLF is important, don't forget it.
                try_expect!(
                    write!(reader.get_mut(), ":localhost PONG {param}\r\n", param=param),
                    "Can't write to the TcpStream"
                );
            },
            message => debug!("=== Message: {:?}", message),
        }

        if registration_finished {
            continue;
        }


        // handle registration
        match (&nick, &user, &real) {
            (Some(ref nick), Some(ref user), Some(_)) => {
                // we have a complete registration from user
                // Send 001, 002, 003, 004, and 005
                send_message!(reader.get_mut(),
                              ":localhost 001 {nick} :Welcome to the Local Network, {nick}!{user}@localhost",
                              nick=nick,
                              user=user
                );
                send_message!(reader.get_mut(),
                              ":localhost 002 {nick} :Your host is localhost[{server}/{port}], running version {programver}",
                              server=SERVER,
                              nick=nick,
                              port=port,
                              programver=PROGRAMVER
                );
                send_message!(reader.get_mut(),
                              ":localhost 003 {nick} :This server was created {datetime}",
                              nick=nick,
                              datetime="Sep 22 2018 at 19:19:32", // fake time
                );
                send_message!(reader.get_mut(),
                              ":localhost 004 {nick} localhost {programver} {usermode} {chanmode} {chanmode_param}",
                              nick=nick,
                              programver=PROGRAMVER,
                              usermode="CDGPRSabcdfgijklnorsuwxyz",
                              chanmode="bciklmnopstvzeIMRS",
                              chanmode_param="bkloveI"
                );
                send_message!(reader.get_mut(),
                              ":localhost 005 {nick} NICKLEN=30 :are supported by this server",
                              nick=nick
                );

                // send this stream to the process_input() after the handshake finished
                if let Ok(stream) = reader.get_ref().try_clone() {
                    let _ = sender.send(stream); // ignore sending error
                }

                // we are done here, don't send this message again
                registration_finished = true;
            },
            _ => (),
        }
    }


    // helper functions

    fn upcase_eq(left: &str, right: &str) -> bool {
        &left.to_ascii_uppercase() == right
    }
}


/// Handle input from the user. Send it to all connected client.
fn process_stdin(receiver: Receiver<TcpStream>) {
    let mut streams = Vec::new();
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        // ignore line error
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };

        // Read all received stream until there's none left in the channel. It's a nonblocking operation,
        // hence we stop reading when the receiver is empty (TryRecvError::Empty).
        loop {
            match receiver.try_recv() {
                Ok(s) => streams.push(s),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    eprintln!("Disconnected receiver");
                    return;
                }
            }
        }

        // send input to all connected stream
        for s in &mut streams {
            send_message!(s, "{}", line);
        }
    }
}


//==== Parsing IRC Message ====
// A rather simple parser for IRC protocol.


/// Simple struct that represents the parsed IRC message.
/// For simplicity sake, we use owned string. It should be possible in theory to use a &str instead.
#[derive(Debug)]
struct IrcMessage {
    tag: Option<String>,
    prefix: Option<String>,
    command: String,
    params: Vec<String>,
}


/// Error type when parsing the message.
#[derive(Debug)]
enum IrcError {
    NoCommand,
}

impl IrcMessage {
    /// Create new IrcMessage from the input string.
    /// Error when there's no command in the message.
    fn new(input: &str) -> Result<Self, IrcError> {
        use IrcError::*;

        let mut parser = IrcParser::new(input);
        let tag = parser.parse_word_if_start_with('@');
        let prefix = parser.parse_word_if_start_with(':');
        let command = match parser.parse_word() {
            Some(command) => command,
            None => return Err(NoCommand),
        };
        let params = parser.parse_params();

        Ok(IrcMessage { tag, prefix, command, params })
    }
}


/// A rather simple parser. We save the input and a marker to the portion of the input that hasn't been read.
struct IrcParser<'a> {
    iter: Peekable<CharIndices<'a>>,
    input: &'a str,
    marker: usize,
}


impl<'a> IrcParser<'a> {
    /// Create new parser from the input.
    /// Also to note, this parser expect the input to not end with "\r\n".
    /// You need to trim them first before passing it to this function.
    fn new(input: &'a str) -> IrcParser<'a> {
        IrcParser {
            iter: input.char_indices().peekable(),
            input: input,
            marker: 0,
        }
    }

    /// Skip whitespace, usually used when reading a word.
    /// Skip any ascii whitespace char until we peek into non-whitespace char.
    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if !c.is_ascii_whitespace() {
                break;
            }
            self.consume_char();
        }
    }

    /// Skip any word. Word in here defined as any non-whitespace character.
    fn skip_word(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                break;
            }
            self.consume_char();
        }
    }

    /// Peek current character.
    fn peek(&mut self) -> Option<char> {
        self.iter.peek().map(|&(_, c)| c)
    }

    /// Consume a single character. Also update the marker.
    fn consume_char(&mut self) {
        self.iter.next();

        if let Some(&(i, _)) = self.iter.peek() {
            self.marker = i;
        } else {
            self.marker = self.input.len();
        }
    }

    /// Parse any word that start with that character. The starting character isn't included.
    fn parse_word_if_start_with(&mut self, start_char: char) -> Option<String> {
        self.skip_whitespace();
        match self.peek() {
            Some(c) if c == start_char => {
                self.consume_char(); // don't include the starting character
                let start = self.marker;
                self.skip_word();
                let end = self.marker;
                Some(self.input[start..end].to_string())
            },
            _ => None,
        }
    }

    /// Parse a single word and return them.
    fn parse_word(&mut self) -> Option<String> {
        self.skip_whitespace();
        let start = self.marker;
        self.skip_word();
        let end = self.marker;
        if start != end {
            Some(self.input[start..end].to_string())
        } else {
            None
        }
    }

    /// Parse params in the irc protocol. Param can be a single word or :rest of the line
    fn parse_params(&mut self) -> Vec<String> {
        let mut params = Vec::new();

        self.skip_whitespace();
        while let Some(c) = self.peek() {
            // :<params> syntax
            if c == ':' {
                params.push(self.input[self.marker+1..].to_string());
                break;
            }
            let start = self.marker;
            self.skip_word();
            let end = self.marker;

            params.push(self.input[start..end].to_string());

            self.skip_whitespace();
        }
        params
    }
}
