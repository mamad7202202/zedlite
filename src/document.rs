use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
}

pub struct Document {
    pub path: Option<PathBuf>,
    pub display_name: String,
    pub lines: Vec<Vec<char>>,
    pub cursor: Cursor,
    pub goal_col: usize,
    pub dirty: bool,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
}

#[derive(Clone)]
struct Snapshot {
    lines: Vec<Vec<char>>,
    cursor: Cursor,
}

impl Document {
    pub fn new_empty() -> Self {
        Self {
            path: None,
            display_name: "untitled".to_string(),
            lines: vec![Vec::new()],
            cursor: Cursor { row: 0, col: 0 },
            goal_col: 0,
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn open(path: PathBuf) -> std::io::Result<Self> {
        let content = fs::read_to_string(&path).unwrap_or_default();
        let display_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let lines = if content.is_empty() {
            vec![Vec::new()]
        } else {
            content
                .split('\n')
                .map(|line| {
                    let line = line.strip_suffix('\r').unwrap_or(line);
                    line.chars().collect()
                })
                .collect()
        };
        Ok(Self {
            path: Some(path),
            display_name,
            lines,
            cursor: Cursor { row: 0, col: 0 },
            goal_col: 0,
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        })
    }

    pub fn set_path(&mut self, path: PathBuf) {
        self.display_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        self.path = Some(path);
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line_text(&self, row: usize) -> String {
        self.lines[row].iter().collect()
    }

    pub fn current_line_len(&self) -> usize {
        self.lines[self.cursor.row].len()
    }

    fn snapshot(&mut self) {
        self.undo_stack.push(Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
        });
        if self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.redo_stack.push(Snapshot {
                lines: self.lines.clone(),
                cursor: self.cursor,
            });
            self.lines = snapshot.lines;
            self.cursor = snapshot.cursor;
            self.goal_col = self.cursor.col;
            self.clamp_cursor();
            self.dirty = true;
        }
    }

    pub fn redo(&mut self) {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push(Snapshot {
                lines: self.lines.clone(),
                cursor: self.cursor,
            });
            self.lines = snapshot.lines;
            self.cursor = snapshot.cursor;
            self.goal_col = self.cursor.col;
            self.clamp_cursor();
            self.dirty = true;
        }
    }

    fn clamp_cursor(&mut self) {
        self.cursor.row = self.cursor.row.min(self.lines.len() - 1);
        self.cursor.col = self.cursor.col.min(self.lines[self.cursor.row].len());
    }

    fn touch(&mut self) {
        self.dirty = true;
    }

    pub fn insert_char(&mut self, ch: char) {
        self.snapshot();
        let row = &mut self.lines[self.cursor.row];
        let col = self.cursor.col.min(row.len());
        row.insert(col, ch);
        self.cursor.col = col + 1;
        self.goal_col = self.cursor.col;
        self.touch();
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.snapshot();
        let segments: Vec<Vec<char>> = text
            .split('\n')
            .map(|part| part.strip_suffix('\r').unwrap_or(part).chars().collect())
            .collect();

        let split_col = self.cursor.col.min(self.lines[self.cursor.row].len());
        let tail = self.lines[self.cursor.row].split_off(split_col);

        let (first, rest) = segments.split_first().unwrap();
        {
            let row = &mut self.lines[self.cursor.row];
            row.splice(split_col..split_col, first.iter().copied());
            self.cursor.col = split_col + first.len();
        }
        for segment in rest {
            self.cursor.row += 1;
            self.lines.insert(self.cursor.row, segment.clone());
            self.cursor.col = segment.len();
        }
        self.lines[self.cursor.row].extend(tail);
        self.goal_col = self.cursor.col;
        self.touch();
    }

    /// Like [`insert_str`] but merges into the previous undo entry.
    pub fn insert_str_no_snapshot(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let segments: Vec<Vec<char>> = text
            .split('\n')
            .map(|part| part.strip_suffix('\r').unwrap_or(part).chars().collect())
            .collect();

        let split_col = self.cursor.col.min(self.lines[self.cursor.row].len());
        let tail = self.lines[self.cursor.row].split_off(split_col);

        let (first, rest) = segments.split_first().unwrap();
        {
            let row = &mut self.lines[self.cursor.row];
            row.splice(split_col..split_col, first.iter().copied());
            self.cursor.col = split_col + first.len();
        }
        for segment in rest {
            self.cursor.row += 1;
            self.lines.insert(self.cursor.row, segment.clone());
            self.cursor.col = segment.len();
        }
        self.lines[self.cursor.row].extend(tail);
        self.goal_col = self.cursor.col;
        self.dirty = true;
    }

    pub fn split_line(&mut self) {
        self.snapshot();
        self.split_line_internal();
        self.touch();
    }

    fn split_line_internal(&mut self) {
        let rest = self.lines[self.cursor.row].split_off(self.cursor.col.min(
            self.lines[self.cursor.row].len(),
        ));
        self.cursor.row += 1;
        self.cursor.col = 0;
        self.goal_col = 0;
        self.lines.insert(self.cursor.row, rest);
    }

    pub fn backspace(&mut self) {
        if self.cursor.col > 0 {
            self.snapshot();
            self.lines[self.cursor.row].remove(self.cursor.col - 1);
            self.cursor.col -= 1;
            self.goal_col = self.cursor.col;
            self.touch();
        } else if self.cursor.row > 0 {
            self.snapshot();
            let line = self.lines.remove(self.cursor.row);
            self.cursor.row -= 1;
            self.cursor.col = self.lines[self.cursor.row].len();
            self.goal_col = self.cursor.col;
            self.lines[self.cursor.row].extend(line);
            self.touch();
        }
    }

    pub fn delete_forward(&mut self) {
        if self.cursor.col < self.lines[self.cursor.row].len() {
            self.snapshot();
            self.lines[self.cursor.row].remove(self.cursor.col);
            self.touch();
        } else if self.cursor.row + 1 < self.lines.len() {
            self.snapshot();
            let next = self.lines.remove(self.cursor.row + 1);
            self.lines[self.cursor.row].extend(next);
            self.touch();
        }
    }

    pub fn delete_line(&mut self) {
        if self.line_count() == 1 {
            self.snapshot();
            self.lines[0].clear();
            self.cursor.col = 0;
            self.goal_col = 0;
            self.touch();
        } else {
            self.snapshot();
            self.lines.remove(self.cursor.row);
            self.clamp_cursor();
            self.cursor.col = self.cursor.col.min(self.current_line_len());
            self.goal_col = self.cursor.col;
            self.touch();
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.cursor.col = self.lines[self.cursor.row].len();
        }
        self.goal_col = self.cursor.col;
    }

    pub fn move_right(&mut self) {
        if self.cursor.col < self.lines[self.cursor.row].len() {
            self.cursor.col += 1;
        } else if self.cursor.row + 1 < self.lines.len() {
            self.cursor.row += 1;
            self.cursor.col = 0;
        }
        self.goal_col = self.cursor.col;
    }

    pub fn move_up(&mut self) {
        if self.cursor.row > 0 {
            self.cursor.row -= 1;
            self.cursor.col = self.goal_col.min(self.lines[self.cursor.row].len());
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor.row + 1 < self.lines.len() {
            self.cursor.row += 1;
            self.cursor.col = self.goal_col.min(self.lines[self.cursor.row].len());
        }
    }

    pub fn page_up(&mut self, amount: usize) {
        self.cursor.row = self.cursor.row.saturating_sub(amount.max(1));
        self.cursor.col = self.goal_col.min(self.lines[self.cursor.row].len());
    }

    pub fn page_down(&mut self, amount: usize) {
        self.cursor.row = (self.cursor.row + amount.max(1)).min(self.lines.len() - 1);
        self.cursor.col = self.goal_col.min(self.lines[self.cursor.row].len());
    }

    pub fn move_home(&mut self) {
        self.cursor.col = 0;
        self.goal_col = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor.col = self.lines[self.cursor.row].len();
        self.goal_col = self.cursor.col;
    }

    pub fn word_left(&mut self) {
        let line = &self.lines[self.cursor.row];
        let mut col = self.cursor.col;
        while col > 0 && !line[col - 1].is_alphanumeric() {
            col -= 1;
        }
        while col > 0 && line[col - 1].is_alphanumeric() {
            col -= 1;
        }
        self.cursor.col = col;
        self.goal_col = col;
    }

    pub fn word_right(&mut self) {
        let line = &self.lines[self.cursor.row];
        let mut col = self.cursor.col;
        while col < line.len() && !line[col].is_alphanumeric() {
            col += 1;
        }
        while col < line.len() && line[col].is_alphanumeric() {
            col += 1;
        }
        self.cursor.col = col;
        self.goal_col = col;
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor.row = row.min(self.lines.len() - 1);
        self.cursor.col = col.min(self.lines[self.cursor.row].len());
        self.goal_col = self.cursor.col;
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        if let Some(path) = &self.path {
            let content: String = self
                .lines
                .iter()
                .map(|line| line.iter().collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(path, content)?;
            self.dirty = false;
        }
        Ok(())
    }

    pub fn language_label(&self) -> &'static str {
        let ext = self
            .path
            .as_ref()
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        match ext {
            "rs" => "Rust",
            "js" | "jsx" | "mjs" => "JavaScript",
            "ts" | "tsx" => "TypeScript",
            "py" => "Python",
            "go" => "Go",
            "rb" => "Ruby",
            "java" => "Java",
            "kt" => "Kotlin",
            "swift" => "Swift",
            "c" | "h" => "C",
            "cpp" | "cc" | "hpp" => "C++",
            "cs" => "C#",
            "json" => "JSON",
            "toml" => "TOML",
            "yaml" | "yml" => "YAML",
            "md" => "Markdown",
            "html" => "HTML",
            "css" => "CSS",
            _ => "Plain Text",
        }
    }
}
