//! gettext plural rules: `nplurals=N; plural=EXPR;` from a `.po` header, with `EXPR` over `n` using integers,
//! `! * / % + - < <= > >= == != && || ?:` and parentheses (C precedence).

/// A header's plural rule. A bad or missing header gives English (`nplurals=2; plural=n != 1;`).
#[derive(Clone, Debug, PartialEq)]
pub struct Plurals { pub count: usize, rule: Expr }

impl Default for Plurals {
    fn default() -> Self { Self { count: 2, rule: Rule::parse("n != 1").expect("English rule").0 } }
}

impl Plurals {
    /// Reads `nplurals=N; plural=EXPR;`. `Err` says what is wrong (the caller falls back to English).
    pub fn parse(header: &str) -> Result<Plurals, String> {
        let mut count = None;
        let mut rule = None;
        for part in header.split(';') {
            let Some((k, v)) = part.split_once('=') else { continue };
            match k.trim() {
                "nplurals" => count = Some(v.trim().parse::<usize>().map_err(|_| format!("nplurals is not a number: {}", v.trim()))?),
                "plural" => rule = Some(Rule::parse(v)?),
                _ => {}
            }
        }
        let count = count.ok_or("no nplurals")?;
        if count == 0 { return Err("nplurals is 0".into()) }
        Ok(Plurals { count, rule: rule.ok_or("no plural rule")?.0 })
    }

    /// `parse`, or English when the header is missing or bad.
    pub fn from_header(header: Option<&str>) -> Plurals { header.and_then(|h| Plurals::parse(h).ok()).unwrap_or_default() }

    /// Which form `n` takes, at most `count - 1`.
    pub fn index(&self, n: u64) -> usize { (eval(&self.rule, n as i64).max(0) as usize).min(self.count - 1) }
}

/// A parsed `plural=` expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule(Expr);

impl Rule {
    pub fn parse(src: &str) -> Result<Rule, String> {
        let toks = lex(src)?;
        let mut p = Parser { toks, at: 0 };
        let e = p.ternary()?;
        if p.at != p.toks.len() { return Err(format!("unexpected {:?} in plural rule", p.toks[p.at])) }
        Ok(Rule(e))
    }

    pub fn eval(&self, n: u64) -> i64 { eval(&self.0, n as i64) }
}

#[derive(Clone, Debug, PartialEq)]
enum Expr {
    N,
    Num(i64),
    Not(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op { Mul, Div, Rem, Add, Sub, Lt, Le, Gt, Ge, Eq, Ne, And, Or }

#[derive(Clone, Debug, PartialEq)]
enum Tok { N, Num(i64), Op(&'static str), Open, Close, Q, Colon }

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() { i += 1; continue }
        if c.is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() { i += 1 }
            out.push(Tok::Num(src[start..i].parse().map_err(|_| "number too big")?));
            continue;
        }
        let two = src.get(i..i + 2).unwrap_or("");
        if let Some(op) = ["<=", ">=", "==", "!=", "&&", "||"].into_iter().find(|o| *o == two) {
            out.push(Tok::Op(op));
            i += 2;
            continue;
        }
        out.push(match c {
            b'n' => Tok::N,
            b'(' => Tok::Open,
            b')' => Tok::Close,
            b'?' => Tok::Q,
            b':' => Tok::Colon,
            b'!' => Tok::Op("!"),
            b'*' => Tok::Op("*"),
            b'/' => Tok::Op("/"),
            b'%' => Tok::Op("%"),
            b'+' => Tok::Op("+"),
            b'-' => Tok::Op("-"),
            b'<' => Tok::Op("<"),
            b'>' => Tok::Op(">"),
            _ => return Err(format!("unexpected character {:?} in plural rule", c as char)),
        });
        i += 1;
    }
    Ok(out)
}

struct Parser { toks: Vec<Tok>, at: usize }

/// Binary operators from loosest to tightest.
const LEVELS: &[&[(&str, Op)]] = &[
    &[("||", Op::Or)],
    &[("&&", Op::And)],
    &[("==", Op::Eq), ("!=", Op::Ne)],
    &[("<", Op::Lt), ("<=", Op::Le), (">", Op::Gt), (">=", Op::Ge)],
    &[("+", Op::Add), ("-", Op::Sub)],
    &[("*", Op::Mul), ("/", Op::Div), ("%", Op::Rem)],
];

impl Parser {
    fn peek(&self) -> Option<&Tok> { self.toks.get(self.at) }

    fn ternary(&mut self) -> Result<Expr, String> {
        let cond = self.binary(0)?;
        if self.peek() != Some(&Tok::Q) { return Ok(cond) }
        self.at += 1;
        let yes = self.ternary()?;
        if self.peek() != Some(&Tok::Colon) { return Err("expected ':' in plural rule".into()) }
        self.at += 1;
        let no = self.ternary()?;
        Ok(Expr::If(Box::new(cond), Box::new(yes), Box::new(no)))
    }

    fn binary(&mut self, level: usize) -> Result<Expr, String> {
        if level == LEVELS.len() { return self.unary() }
        let mut left = self.binary(level + 1)?;
        while let Some(Tok::Op(s)) = self.peek() {
            let Some(&(_, op)) = LEVELS[level].iter().find(|(o, _)| o == s) else { break };
            self.at += 1;
            let right = self.binary(level + 1)?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.toks.get(self.at).cloned() {
            Some(Tok::Op("!")) => { self.at += 1; Ok(Expr::Not(Box::new(self.unary()?))) }
            Some(Tok::N) => { self.at += 1; Ok(Expr::N) }
            Some(Tok::Num(v)) => { self.at += 1; Ok(Expr::Num(v)) }
            Some(Tok::Open) => {
                self.at += 1;
                let e = self.ternary()?;
                if self.peek() != Some(&Tok::Close) { return Err("expected ')' in plural rule".into()) }
                self.at += 1;
                Ok(e)
            }
            Some(t) => Err(format!("unexpected {t:?} in plural rule")),
            None => Err("plural rule ends too early".into()),
        }
    }
}

fn eval(e: &Expr, n: i64) -> i64 {
    match e {
        Expr::N => n,
        Expr::Num(v) => *v,
        Expr::Not(a) => (eval(a, n) == 0) as i64,
        Expr::If(c, y, no) => if eval(c, n) != 0 { eval(y, n) } else { eval(no, n) },
        Expr::Bin(op, a, b) => {
            let (a, b) = (eval(a, n), eval(b, n));
            match op {
                Op::Mul => a.wrapping_mul(b),
                Op::Div => if b == 0 { 0 } else { a / b },
                Op::Rem => if b == 0 { 0 } else { a % b },
                Op::Add => a.wrapping_add(b),
                Op::Sub => a.wrapping_sub(b),
                Op::Lt => (a < b) as i64,
                Op::Le => (a <= b) as i64,
                Op::Gt => (a > b) as i64,
                Op::Ge => (a >= b) as i64,
                Op::Eq => (a == b) as i64,
                Op::Ne => (a != b) as i64,
                Op::And => (a != 0 && b != 0) as i64,
                Op::Or => (a != 0 || b != 0) as i64,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forms(header: &str, ns: &[u64]) -> Vec<usize> {
        let p = Plurals::parse(header).unwrap();
        ns.iter().map(|&n| p.index(n)).collect()
    }

    #[test]
    fn english_and_french() {
        assert_eq!(forms("nplurals=2; plural=(n != 1);", &[0, 1, 2, 5]), [1, 0, 1, 1]);
        assert_eq!(forms("nplurals=2; plural=(n > 1);", &[0, 1, 2]), [0, 0, 1]);
    }

    #[test]
    fn russian_and_ukrainian_three_forms() {
        let ru = "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);";
        assert_eq!(forms(ru, &[1, 2, 5, 11, 12, 21, 22, 25, 101, 111]), [0, 1, 2, 2, 2, 0, 1, 2, 0, 2]);
    }

    #[test]
    fn polish() {
        let pl = "nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);";
        assert_eq!(forms(pl, &[1, 2, 5, 12, 22, 25, 0]), [0, 1, 2, 2, 1, 2, 2]);
    }

    #[test]
    fn arabic_six_forms() {
        let ar = "nplurals=6; plural=(n==0 ? 0 : n==1 ? 1 : n==2 ? 2 : n%100>=3 && n%100<=10 ? 3 : n%100>=11 ? 4 : 5);";
        assert_eq!(forms(ar, &[0, 1, 2, 3, 10, 11, 99, 100, 102, 103]), [0, 1, 2, 3, 3, 4, 4, 5, 5, 3]);
    }

    #[test]
    fn japanese_one_form() {
        assert_eq!(forms("nplurals=1; plural=0;", &[0, 1, 7]), [0, 0, 0]);
    }

    #[test]
    fn not_and_precedence() {
        assert_eq!(Rule::parse("!(n == 1)").unwrap().eval(1), 0);
        assert_eq!(Rule::parse("1 + 2 * 3").unwrap().eval(0), 7);
        assert_eq!(Rule::parse("n - 1 - 1").unwrap().eval(5), 3);
        assert_eq!(Rule::parse("n % 0").unwrap().eval(5), 0);
    }

    #[test]
    fn bad_headers_fall_back_to_english() {
        for bad in ["", "nplurals=2;", "plural=n!=1;", "nplurals=x; plural=n;", "nplurals=2; plural=(n !=;", "nplurals=2; plural=n $ 1;", "nplurals=0; plural=0;"] {
            assert!(Plurals::parse(bad).is_err(), "{bad}");
            assert_eq!(Plurals::from_header(Some(bad)), Plurals::default());
        }
        assert_eq!(Plurals::from_header(None).index(1), 0);
        assert_eq!(Plurals::from_header(None).index(3), 1);
        // A rule that answers past the last form is held to it.
        assert_eq!(Plurals::parse("nplurals=2; plural=n;").unwrap().index(9), 1);
    }
}
