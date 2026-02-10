use std::rc::Rc;

#[derive(Clone)]
pub enum AstNode {
    Text(Rc<String>),
    Command(AstCommand),
}

pub type Ast = Vec<AstNode>;

#[derive(Clone)]
pub struct AstCommand {
    pub name: Rc<String>,
    /// 每个参数的 AST 表示
    pub args: Vec<Ast>,
}

pub fn parse_to_ast(input: &str) -> Result<Ast, String> {
    let mut parser = AstParser::new(input);
    parser.parse()
}

/// 将 Ast 序列化回字面量字符串
pub fn ast_to_string(ast: &Ast) -> String {
    let mut out = String::new();
    for node in ast {
        match node {
            AstNode::Text(text) => {
                for ch in text.chars() {
                    if ch == '\\' || ch == '@' || ch == '【' || ch == '】' {
                        out.push('\\');
                    }
                    out.push(ch);
                }
            }
            AstNode::Command(cmd) => {
                out.push('【');
                for ch in cmd.name.chars() {
                    if ch == '\\' || ch == '@' || ch == '【' || ch == '】' {
                        out.push('\\');
                    }
                    out.push(ch);
                }
                for arg in cmd.args.iter() {
                    out.push('@');
                    out.push_str(&ast_to_string(arg));
                }
                out.push('】');
            }
        }
    }
    out
}

/// 将一个已求值的字符串包装为 Ast（纯文本节点）
pub fn str_to_ast(s: String) -> Ast {
    vec![AstNode::Text(Rc::new(s))]
}

struct AstParser {
    chars: Vec<char>,
}

impl AstParser {
    fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
        }
    }
    fn remove_comment(&mut self) -> Result<(), String> {
        let mut read_pos = 0;
        let mut write_pos = 0;
        let mut in_comment = false;

        while read_pos < self.chars.len() {
            let curr_char = self.chars[read_pos];
            if in_comment {
                if curr_char == '\n' {
                    self.chars[write_pos] = curr_char;
                    write_pos += 1;
                    in_comment = false;
                }
                read_pos += 1;
            } else {
                if curr_char == '\\' {
                    let next = *self.chars.get(read_pos + 1).ok_or_else(|| "期望转义字符，但输入结束".to_string())?;
                    self.chars[write_pos] = curr_char;
                    self.chars[write_pos + 1] = next;
                    write_pos += 2;
                    read_pos += 2;
                } else if curr_char == '#' && self.chars.get(read_pos + 1) == Some(&'#') {
                    in_comment = true;
                    read_pos += 2;
                } else {
                    self.chars[write_pos] = curr_char;
                    write_pos += 1;
                    read_pos += 1;
                }
            }
        }
        self.chars.truncate(write_pos);
        Ok(())
    }
    fn parse(&mut self) -> Result<Ast, String> {
        self.remove_comment()?;
        let mut parser = AstParserInner::new(&self.chars);
        parser.parse()
    }
}

struct AstParserInner<'a> {
    chars: &'a [char],
    pos: usize,
}

impl<'a> AstParserInner<'a> {
    fn new(input: &'a [char]) -> Self {
        Self {
            chars: input,
            pos: 0,
        }
    }

    fn consume_char(&mut self, num: usize) -> Result<(), String> {
        if self.pos + num > self.chars.len() {
            return Err(format!("期望 {} 个字符，但输入结束", num));
        }
        self.pos += num;
        Ok(())
    }

    fn look_ahead(&self, num: usize) -> Result<char, String> {
        if self.pos + num >= self.chars.len() {
            return Err(format!("期望 {} 个字符，但输入结束", num));
        }
        Ok(self.chars[self.pos + num])
    }

    /// 解析命令参数，返回每个参数的 AST
    fn parse_command_args(&mut self) -> Result<Vec<Ast>, String> {
        let mut args = Vec::new();
        let mut temp_start = self.pos;
        let mut s_num = 0usize;
        let mut g_num = 0usize;
        let mut is_normal_break = false;
        enum ArgState {
            RawText,
            NotRawText,
        }
        let mut arg_state = ArgState::NotRawText;
        while self.pos < self.chars.len() {
            let curr_char = self.chars[self.pos];
            match arg_state {
                ArgState::NotRawText => {
                    if curr_char == '\\' {
                        self.look_ahead(1)?;
                        self.consume_char(2)?;
                    } else if curr_char == '【' {
                        if self.look_ahead(1)? == '@' {
                            arg_state = ArgState::RawText;
                            self.consume_char(2)?;
                            s_num = 1;
                        } else {
                            g_num += 1;
                            self.consume_char(1)?;
                        }
                    } else if curr_char == '】' {
                        if g_num == 0 {
                            let mut arg_parser = AstParserInner::new(&self.chars[temp_start..self.pos]);
                            args.push(arg_parser.parse()?);
                            self.consume_char(1)?;
                            is_normal_break = true;
                            break;
                        }
                        g_num -= 1;
                        self.consume_char(1)?;
                    } else if curr_char == '@' {
                        if g_num == 0 {
                            let mut arg_parser = AstParserInner::new(&self.chars[temp_start..self.pos]);
                            args.push(arg_parser.parse()?);
                            self.consume_char(1)?;
                            temp_start = self.pos;
                        } else {
                            self.consume_char(1)?;
                        }
                    } else {
                        self.consume_char(1)?;
                    }
                },
                ArgState::RawText => {
                    if curr_char == '】' {
                        if s_num == 0 {
                            return Err("未匹配的结束括号 '】'".to_string());
                        }
                        s_num -= 1;
                        if s_num == 0 {
                            arg_state = ArgState::NotRawText;
                        }
                    } else if curr_char == '【' {
                        s_num += 1;
                    } else {
                        // do nothing
                    }
                    self.consume_char(1)?;
                }
            }
        }
        if s_num != 0 {
            return Err(format!("未闭合的原始字符串 '【@'"));
        }
        if !is_normal_break {
            return Err("未闭合的开始括号 '【'".to_string());
        }
        Ok(args)
    }

    fn parse(&mut self) -> Result<Ast, String> {
        let mut result = Vec::new();

        enum State {
            Text,
            RawText,
            CommandName,
            CommandArgs,
        }

        let mut state = State::Text;
        let mut text = String::new();
        let mut command_name = String::new();

        let mut s_num = 0usize;
        
        while self.pos < self.chars.len() {
            let curr_char = self.chars[self.pos];
            match state {
                State::Text => {
                    if curr_char == '【' {
                        match self.look_ahead(1)? {
                            '@' => {
                                self.consume_char(2)?;
                                s_num = 1;
                                state = State::RawText;
                            },
                            _ => {
                                self.consume_char(1)?;
                                state = State::CommandName;
                                command_name.clear();
                                if !text.is_empty() {
                                    result.push(AstNode::Text(Rc::new(std::mem::take(&mut text))));
                                }
                            }
                        }
                    } else if curr_char == '\\' {
                        text.push(self.look_ahead(1)?);
                        self.consume_char(2)?;
                    }
                    else {
                        if !curr_char.is_whitespace() {
                            text.push(curr_char);
                        }
                        self.consume_char(1)?;
                    }
                },
                State::RawText => {
                    if curr_char == '】' {
                        if s_num == 0 {
                            return Err("未匹配的结束括号 '】'".to_string());
                        }
                        s_num -= 1;
                        if s_num == 0 {
                            state = State::Text;
                        } else {
                            text.push(curr_char);
                        }
                    } else if curr_char == '【' {
                        s_num += 1;
                        text.push(curr_char);
                    } else {
                        text.push(curr_char);
                    }
                    self.consume_char(1)?;
                }
                State::CommandName => {
                    if curr_char == '@' {
                        state = State::CommandArgs;
                        self.consume_char(1)?;
                    } else if curr_char == '】' {
                        state = State::Text;
                        result.push(AstNode::Command(AstCommand {
                            name: Rc::new(std::mem::take(&mut command_name)),
                            args: Vec::new(),
                        }));
                        self.consume_char(1)?;
                    } else if curr_char == '【' {
                        state = State::CommandArgs;
                    }
                    else {
                        if !curr_char.is_whitespace() {
                            command_name.push(curr_char);
                        }
                        self.consume_char(1)?;
                    }
                    
                },
                State::CommandArgs => {
                    let args = self.parse_command_args()?;
                    result.push(AstNode::Command(AstCommand {
                        name: Rc::new(std::mem::take(&mut command_name)),
                        args,
                    }));
                    state = State::Text;
                }
            }
        }

        match state {
            State::Text => {
                if !text.is_empty() {
                    result.push(AstNode::Text(Rc::new(text)));
                }
            },
            State::RawText => return Err("未闭合的原始字符串 '【@'".to_string()),
            State::CommandName => return Err("未闭合的命令名 '【'".to_string()),
            State::CommandArgs => return Err("未闭合的命令参数 '【'".to_string()),
        }

        if result.is_empty() {
            result.push(AstNode::Text(Rc::new("".to_owned())));
        }
        Ok(result)
    }

}
