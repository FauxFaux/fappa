use conch_parser::ast::DefaultWord;
use conch_parser::ast::SimpleWord;
use failure::bail;
use failure::ensure;
use failure::err_msg;
use failure::format_err;
use failure::Error;
use failure::ResultExt;

pub fn load(from: &str) -> Result<Vec<Vec<String>>, Error> {
    split(from)
        .into_iter()
        .map(
            |block| Ok(tokens(&block).with_context(|_| format_err!("parsing block: {:?}", block))?),
        )
        .collect()
}

fn split(from: &str) -> Vec<String> {
    from.split("\n\n")
        .map(|p| strip_comments(p))
        .filter(|s| !s.trim().is_empty())
        .collect()
}

fn strip_comments(from: &str) -> String {
    from.trim()
        .lines()
        .filter(|l| !l.trim().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

fn tokens(from: &str) -> Result<Vec<String>, Error> {
    use conch_parser::ast::Command;
    use conch_parser::ast::ComplexWord;
    use conch_parser::ast::ListableCommand;
    use conch_parser::ast::PipeableCommand;
    use conch_parser::ast::RedirectOrCmdWord;
    use conch_parser::ast::TopLevelCommand;
    use conch_parser::ast::TopLevelWord;
    use conch_parser::lexer::Lexer;
    use conch_parser::parse::DefaultParser;

    let mut cmds = DefaultParser::new(Lexer::new(from.chars())).into_iter();

    let cmd: TopLevelCommand<_> = cmds.next().ok_or_else(|| err_msg("no command"))??;

    ensure!(cmds.next().is_none(), "multiple commands in one block");

    let cmd = cmd.0;
    let cmd = match cmd {
        Command::List(l) => l,
        Command::Job(_) => bail!("jobs not supported"),
    };

    ensure!(cmd.rest.is_empty(), "rest not supported");

    let cmd = cmd.first;

    let cmd = match cmd {
        ListableCommand::Single(s) => s,
        ListableCommand::Pipe(_, _) => bail!("pipes not supported"),
    };

    let cmd = match cmd {
        PipeableCommand::Simple(s) => s,
        other => bail!("command types not supported: {:?}", other),
    };

    ensure!(
        cmd.redirects_or_env_vars.is_empty(),
        "no redirects or env vars"
    );

    let cmd = cmd.redirects_or_cmd_words;

    cmd.into_iter()
        .map(|c| {
            match c {
                RedirectOrCmdWord::Redirect(_) => bail!("redirect not supported"),
                RedirectOrCmdWord::CmdWord(w) => Ok(w),
            }
            .and_then(|w| match &*w {
                ComplexWord::Single(w) => literal_word(w),
                other => bail!("complex word {:?}", other),
            })
        })
        .collect()
}

fn literal_word(word: &DefaultWord) -> Result<String, Error> {
    use conch_parser::ast::Word;
    Ok(match word {
        Word::SingleQuoted(w) => w.to_string(),
        Word::DoubleQuoted(w) => bail!("unsupported double quotes"),
        Word::Simple(w) => match w {
            SimpleWord::Literal(s) => s.to_string(),
            other => bail!("unsupported simple word {:?}", other),
        },
    })
}

#[test]
fn tokenizer() {
    assert_eq!(
        vec!["foo", "bar", "baz quux", "baz potato"],
        tokens("foo \\\n  bar 'baz quux' 'baz potato'").unwrap()
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

#[test]
fn full_load() {
    load(include_str!("../specs/sigrok.sh")).unwrap();
}
