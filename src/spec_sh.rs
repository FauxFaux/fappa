use failure::bail;
use failure::Error;

fn split(from: &str) -> Vec<String> {
    from.split("\n\n").map(|p| strip_comments(p)).collect()
}

fn strip_comments(from: &str) -> String {
    from.trim()
        .lines()
        .filter(|l| !l.trim().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

fn tokens(from: &str) -> Result<Vec<String>, Error> {
    use conch_parser::lexer::Lexer;
    use conch_parser::parse::DefaultParser;
    use conch_parser::token::Token;

    let mut ret = Vec::new();
    let mut tokens = Lexer::new(from.chars()).peekable();
    let mut prev_slash = false;
    while let Some(val) = tokens.next() {
        match val {
            Token::Name(n) => ret.push(n),
            Token::Backslash if tokens.peek() == Some(&Token::Newline) => {
                prev_slash = true;
            }
            Token::Newline if prev_slash => {
                prev_slash = false;
            }
            Token::Whitespace(_) => {
                prev_slash = false;
            }
            other => bail!("unsupported token: {:?}", other),
        }
    }

    Ok(ret)
}

#[test]
fn tokenizer() {
    assert_eq!(
        vec![""],
        tokens("foo \\\n  bar 'baz quux' \"lol\"").unwrap()
    )
}

#[test]
fn splitting() {
    assert_eq!(
        vec!["foo \\\n  bar", "baz \\\n  quux"],
        split(
            r"
# first, let's do something
foo \
  bar

# then, something else?
# maybe
baz \
  quux
"
        )
    )
}
