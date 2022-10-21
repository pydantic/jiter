fn main() {
    let data = b"[\"foo\", 123, {\"foobar\": true}]";
    let mut chunker = Chunker::new(&data[..]);
    loop {
        let chunk = chunker.next().unwrap();
        println!("error: {:?}", chunk);
        if matches!(chunk.chunk_type, ChunkType::End) {
            break;
        }
    }
}

#[derive(Debug)]
enum ErrorType {
    UnexpectedCharacter,
    UnexpectedEnd,
    ExpectingColon,
    ExpectingArrayNext,
    ExpectingObjectNext,
    ExpectingKey,
    ExpectingValue,
    InvalidTrue,
    InvalidFalse,
    InvalidNull,
    InvalidString(usize),
    InvalidNumber,
}

#[derive(Debug)]
struct Error {
    error_type: ErrorType,
    line: usize,
    col: usize,
}

#[derive(Debug)]
struct Exponent {
    positive: bool,
    range: (usize, usize),
}

#[derive(Debug)]
enum ChunkType {
    End,
    ObjectStart,
    ObjectEnd,
    ArrayStart,
    ArrayEnd,
    True,
    False,
    Null,
    String(usize, usize),
    Int {
        positive: bool,
        range: (usize, usize),
    },
    IntExponent {
        positive: bool,
        range: (usize, usize),
        exponent: Exponent,
    },
    Float {
        positive: bool,
        range: (usize, usize, usize),
    },
    FloatExponent {
        positive: bool,
        range: (usize, usize, usize),
        exponent: Exponent,
    },
}

#[derive(Debug)]
struct Chunk {
    key: Option<(usize, usize)>,
    chunk_type: ChunkType,
    line: usize,
    col: usize,
}

type ChunkerResult<T> = Result<T, ErrorType>;

#[derive(Debug, Copy, Clone)]
enum State {
    Start,
    StartArray,
    MidArray,
    StartObject,
    MidObject,
}

#[derive(Debug)]
struct Chunker<'a> {
    data: &'a [u8],
    length: usize,
    state_heap: Vec<State>,
    state: State,
    started: bool,
    index: usize,
    line: usize,
    col_offset: usize,
}

impl<'a> Chunker<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        return Self {
            data,
            length: data.len(),
            state_heap: vec![],
            state: State::Start,
            started: false,
            index: 0,
            line: 0,
            col_offset: 0,
        };
    }

    pub fn next(&mut self) -> Result<Chunk, Error> {
        self.eat_whitespace();

        let result = match self.state {
            State::Start => self.parse_next(),
            State::StartArray => self.array_start(),
            State::MidArray => self.array_mid(),
            State::StartObject => self.object_start(),
            State::MidObject => self.object_mid(),
        };

        let col = self.index - self.col_offset;
        match result {
            Ok((key, chunk_type)) => Ok(Chunk {
                key,
                chunk_type,
                line: self.line,
                col,
            }),
            Err(error_type) => Err(Error {
                error_type,
                line: self.line,
                col,
            }),
        }
    }

    fn eat_whitespace(&mut self) {
        while self.index < self.length {
            let next = unsafe { self.data.get_unchecked(self.index) };
            match next {
                b' ' | b'\r' | b'\t' => self.index += 1,
                b'\n' => {
                    self.index += 1;
                    self.line += 1;
                    self.col_offset = self.index;
                }
                _ => break,
            }
        }
    }

    // if we're in an array consume the next comma and whitespace
    fn array_start(&mut self) -> ChunkerResult<(Option<(usize, usize)>, ChunkType)> {
        if self.next_is(b']')? {
            self.index += 1;
            self.state = self.state_heap.pop().unwrap();
            Ok((None, ChunkType::ArrayEnd))
        } else {
            self.state = State::MidArray;
            self.parse_next()
        }
    }

    // if we're in an array consume the next comma and whitespace
    fn array_mid(&mut self) -> ChunkerResult<(Option<(usize, usize)>, ChunkType)> {
        if self.index >= self.length {
            Err(ErrorType::UnexpectedEnd)
        } else {
            let next = unsafe { self.data.get_unchecked(self.index) };
            if next == &b']' {
                self.index += 1;
                self.state = self.state_heap.pop().unwrap();
                Ok((None, ChunkType::ArrayEnd))
            } else if next == &b',' {
                self.index += 1;
                self.eat_whitespace();
                self.parse_next()
            } else {
                Err(ErrorType::ExpectingArrayNext)
            }
        }
    }

    fn object_start(&mut self) -> ChunkerResult<(Option<(usize, usize)>, ChunkType)> {
        if self.next_is(b'}')? {
            self.index += 1;
            self.state = self.state_heap.pop().unwrap();
            Ok((None, ChunkType::ObjectEnd))
        } else {
            self.state = State::MidObject;
            self.object_next()
        }
    }

    fn object_mid(&mut self) -> ChunkerResult<(Option<(usize, usize)>, ChunkType)> {
        if self.index >= self.length {
            Err(ErrorType::UnexpectedEnd)
        } else {
            let next = unsafe { self.data.get_unchecked(self.index) };
            if next == &b'}' {
                self.index += 1;
                self.state = self.state_heap.pop().unwrap();
                Ok((None, ChunkType::ObjectEnd))
            } else if next == &b',' {
                self.index += 1;
                self.eat_whitespace();
                self.object_next()
            } else {
                Err(ErrorType::ExpectingObjectNext)
            }
        }
    }

    fn object_next(&mut self) -> ChunkerResult<(Option<(usize, usize)>, ChunkType)> {
        if self.next_is(b'"')? {
            let (key_start, key_end) = self.parse_string()?;
            self.eat_whitespace();
            if self.next_is(b':')? {
                self.index += 1;
                self.eat_whitespace();
                let (_, value) = self.parse_next()?;
                Ok((Some((key_start, key_end)), value))
            } else {
                Err(ErrorType::ExpectingColon)
            }
        } else {
            Err(ErrorType::ExpectingKey)
        }
    }

    fn parse_next(&mut self) -> ChunkerResult<(Option<(usize, usize)>, ChunkType)> {
        if self.index >= self.length {
            return match self.state {
                State::Start => {
                    if self.started {
                        Ok((None, ChunkType::End))
                    } else {
                        Err(ErrorType::UnexpectedEnd)
                    }
                }
                _ => Err(ErrorType::UnexpectedEnd),
            };
        }

        let next = unsafe { self.data.get_unchecked(self.index) };
        let chunk_type = match next {
            b'{' => {
                self.index += 1;
                self.state_heap.push(self.state);
                self.state = State::StartObject;
                Ok(ChunkType::ObjectStart)
            }
            b'[' => {
                self.index += 1;
                self.state_heap.push(self.state);
                self.state = State::StartArray;
                Ok(ChunkType::ArrayStart)
            }
            b't' => self.parse_true(),
            b'f' => self.parse_false(),
            b'n' => self.parse_null(),
            b'"' => {
                let (start, end) = self.parse_string()?;
                Ok(ChunkType::String(start, end))
            }
            b'0'..=b'9' => self.parse_number(),
            _ => Err(ErrorType::UnexpectedCharacter),
        }?;
        self.started = true;
        Ok((None, chunk_type))
    }

    fn parse_true(&mut self) -> ChunkerResult<ChunkType> {
        if self.index + 3 >= self.length {
            return Err(ErrorType::UnexpectedEnd);
        }
        // this could be a SIMD operation and possibly faster?
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'r' {
            return Err(ErrorType::InvalidTrue);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'u' {
            return Err(ErrorType::InvalidTrue);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'e' {
            return Err(ErrorType::InvalidTrue);
        }
        self.index += 1;
        Ok(ChunkType::True)
    }

    fn parse_false(&mut self) -> ChunkerResult<ChunkType> {
        if self.index + 4 >= self.length {
            return Err(ErrorType::UnexpectedEnd);
        }
        // this could be a SIMD operation and possibly faster?
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'a' {
            return Err(ErrorType::InvalidFalse);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'l' {
            return Err(ErrorType::InvalidFalse);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b's' {
            return Err(ErrorType::InvalidFalse);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'e' {
            return Err(ErrorType::InvalidFalse);
        }
        self.index += 1;
        Ok(ChunkType::False)
    }

    fn parse_null(&mut self) -> ChunkerResult<ChunkType> {
        if self.index + 3 >= self.length {
            return Err(ErrorType::UnexpectedEnd);
        }
        // this could be a SIMD operation and possibly faster?
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'u' {
            return Err(ErrorType::InvalidNull);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'l' {
            return Err(ErrorType::InvalidNull);
        }
        self.index += 1;
        let next = unsafe { self.data.get_unchecked(self.index) };
        if next != &b'l' {
            return Err(ErrorType::InvalidNull);
        }
        self.index += 1;
        Ok(ChunkType::Null)
    }

    fn parse_string(&mut self) -> ChunkerResult<(usize, usize)> {
        self.index += 1;
        let start = self.index;
        while self.index < self.length {
            let next = unsafe { self.data.get_unchecked(self.index) };
            match next {
                b'"' => {
                    let r = (start, self.index);
                    self.index += 1;
                    return Ok(r);
                }
                b'\\' => {
                    self.index += 1;
                    if self.index >= self.length {
                        break;
                    }
                    let next = unsafe { self.data.get_unchecked(self.index) };
                    match next {
                        // we don't check the 4 digit unicode escape sequence here, we just move on
                        b'"' | b'\\' | b'/' | b'u' => (),
                        // 8 = backspace, 9 = tab, 10 = newline, 12 = formfeed, 13 = carriage return
                        8 | 9 | 10 | 12 | 13 => (),
                        _ => return Err(self.string_error(start)),
                    }
                }
                // 8 = backspace, 9 = tab, 10 = newline, 12 = formfeed, 13 = carriage return
                8 | 9 | 10 | 12 | 13 => return Err(self.string_error(start)),
                _ => (),
            }
            self.index += 1;
        }
        Err(ErrorType::UnexpectedEnd)
    }

    fn string_error(&mut self, start: usize) -> ErrorType {
        let location = self.index - start;
        // reset index so the error appears at the start of the string
        self.index = start;
        ErrorType::InvalidString(location)
    }

    fn parse_number(&mut self) -> ChunkerResult<ChunkType> {
        let mut start = self.index;
        let mut first = true;
        let mut positive = true;
        self.index += 1;
        while self.index < self.length {
            let next = unsafe { self.data.get_unchecked(self.index) };
            match next {
                b'-' => {
                    if !first {
                        return Err(ErrorType::InvalidNumber);
                    }
                    positive = false;
                    start += 1;
                }
                b'0'..=b'9' => (),
                b'.' => {
                    return if first {
                        Err(ErrorType::InvalidNumber)
                    } else {
                        self.float_decimal(start, positive)
                    }
                }
                b'e' | b'E' => {
                    return if first {
                        Err(ErrorType::InvalidNumber)
                    } else {
                        let exponent = self.exponent()?;
                        Ok(ChunkType::IntExponent {
                            positive,
                            range: (start, self.index),
                            exponent,
                        })
                    }
                }
                _ => break,
            }
            first = false;
            self.index += 1;
        }
        Ok(ChunkType::Int {
            positive,
            range: (start, self.index),
        })
    }

    fn float_decimal(&mut self, start: usize, positive: bool) -> ChunkerResult<ChunkType> {
        let mut first = true;
        self.index += 1;
        let mut decimal_start = self.index;
        while self.index < self.length {
            let next = unsafe { self.data.get_unchecked(self.index) };
            match next {
                b'0'..=b'9' => (),
                b'e' | b'E' => {
                    return if first {
                        Err(ErrorType::InvalidNumber)
                    } else {
                        let exponent = self.exponent()?;
                        Ok(ChunkType::FloatExponent {
                            positive,
                            range: (start, decimal_start, self.index),
                            exponent,
                        })
                    }
                }
                _ => break,
            }
            first = false;
            self.index += 1;
        }
        if decimal_start == self.index {
            Err(ErrorType::InvalidNumber)
        } else {
            Ok(ChunkType::Float {
                positive,
                range: (start, decimal_start, self.index),
            })
        }
    }

    fn exponent(&mut self) -> ChunkerResult<Exponent> {
        let mut first = true;
        let mut positive = true;
        self.index += 1;
        let mut start = self.index;
        while self.index < self.length {
            let next = unsafe { self.data.get_unchecked(self.index) };
            match next {
                b'-' => {
                    if !first {
                        return Err(ErrorType::InvalidNumber);
                    }
                    positive = false;
                    start += 1;
                }
                b'+' => {
                    if !first {
                        return Err(ErrorType::InvalidNumber);
                    }
                    start += 1;
                }
                b'0'..=b'9' => (),
                _ => break,
            }
            first = false;
            self.index += 1;
        }

        if start == self.index {
            Err(ErrorType::InvalidNumber)
        } else {
            Ok(Exponent {
                positive,
                range: (start, self.index),
            })
        }
    }

    fn next_is(&self, byte: u8) -> ChunkerResult<bool> {
        if self.index >= self.length {
            Err(ErrorType::UnexpectedEnd)
        } else {
            let next = unsafe { self.data.get_unchecked(self.index) };
            Ok(next == &byte)
        }
    }
}
