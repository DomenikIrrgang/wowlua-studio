//! A line diff (Myers' algorithm) for comparing the editor's text with the
//! disk's (S049); source control's diffs start from it (M11).

/// A line of a diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line<'a> {
    Same(&'a str),
    /// Only in the first text: mine.
    Removed(&'a str),
    /// Only in the second: theirs.
    Added(&'a str),
}

/// The lines of `a` and `b` in order, each kept, removed or added; `None`
/// when they differ too much to compare quickly.
pub fn lines<'a>(a: &'a str, b: &'a str) -> Option<Vec<Line<'a>>> {
    let a: Vec<&str> = a.split('\n').collect();
    let b: Vec<&str> = b.split('\n').collect();
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (n + m) as usize;
    // At most 2000 differing lines: beyond that a diff is no help.
    let limit = max.min(2000);
    let offset = max as isize + 1;
    let mut v = vec![0isize; 2 * max + 3];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = false;
    for d in 0..=limit as isize {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let i = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[i - 1] < v[i + 1]) {
                v[i + 1]
            } else {
                v[i - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[i] = x;
            if x >= n && y >= m {
                found = true;
                break;
            }
            k += 2;
        }
        if found {
            break;
        }
    }
    if !found {
        return None;
    }
    // Walk back through the trace.
    let mut out = Vec::new();
    let (mut x, mut y) = (n, m);
    for (d, v) in trace.iter().enumerate().rev() {
        let d = d as isize;
        let k = x - y;
        let i = (k + offset) as usize;
        let prev_k = if k == -d || (k != d && v[i - 1] < v[i + 1]) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = v[(prev_k + offset) as usize];
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            out.push(Line::Same(a[x as usize]));
        }
        if d > 0 {
            if x == prev_x {
                y -= 1;
                out.push(Line::Added(b[y as usize]));
            } else {
                x -= 1;
                out.push(Line::Removed(a[x as usize]));
            }
        }
    }
    out.reverse();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_line_is_removed_and_added() {
        let diff = lines("a\nb\nc", "a\nx\nc\nd").unwrap();
        assert_eq!(
            diff,
            [
                Line::Same("a"),
                Line::Removed("b"),
                Line::Added("x"),
                Line::Same("c"),
                Line::Added("d"),
            ]
        );
    }

    #[test]
    fn equal_texts_are_the_same_lines() {
        assert_eq!(
            lines("a\nb", "a\nb").unwrap(),
            [Line::Same("a"), Line::Same("b")]
        );
    }
}
