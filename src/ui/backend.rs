use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::style::{ContentStyle, SetStyle};
use crossterm::terminal::{self, Clear as CrosstermClear, ClearType as CrosstermClearType};
use ratatui::backend::{Backend, ClearType, IntoCrossterm, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};

use crate::ui::links::LinkRegistry;

/// Crossterm backend that wraps each linked cell in an OSC 8 hyperlink.
pub struct LinkBackend {
    out: io::Stdout,
    links: LinkRegistry,
}

impl LinkBackend {
    pub fn new(links: LinkRegistry) -> Self {
        Self {
            out: io::stdout(),
            links,
        }
    }
}

impl Backend for LinkBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            let style: ContentStyle = cell.style().into_crossterm();
            crossterm::queue!(self.out, MoveTo(x, y), SetStyle(style))?;

            let link = self.links.get(x, y);
            if let Some(url) = &link {
                write!(self.out, "\u{1b}]8;;{url}\u{1b}\\")?;
            }
            write!(self.out, "{}", cell.symbol())?;
            if link.is_some() {
                write!(self.out, "\u{1b}]8;;\u{1b}\\")?;
            }
        }
        self.out.flush()
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        crossterm::queue!(self.out, Hide)
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        crossterm::queue!(self.out, Show)
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        let (x, y) = crossterm::cursor::position()?;
        Ok(Position { x, y })
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let position = position.into();
        crossterm::queue!(self.out, MoveTo(position.x, position.y))
    }

    fn clear(&mut self) -> io::Result<()> {
        crossterm::queue!(self.out, CrosstermClear(CrosstermClearType::All))
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        let clear_type = match clear_type {
            ClearType::All => CrosstermClearType::All,
            ClearType::AfterCursor => CrosstermClearType::FromCursorDown,
            ClearType::BeforeCursor => CrosstermClearType::FromCursorUp,
            ClearType::CurrentLine => CrosstermClearType::CurrentLine,
            ClearType::UntilNewLine => CrosstermClearType::UntilNewLine,
        };
        crossterm::queue!(self.out, CrosstermClear(clear_type))
    }

    fn size(&self) -> io::Result<Size> {
        let (width, height) = terminal::size()?;
        Ok(Size { width, height })
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        let size = terminal::window_size()?;
        Ok(WindowSize {
            columns_rows: Size {
                width: size.columns,
                height: size.rows,
            },
            pixels: Size {
                width: size.width,
                height: size.height,
            },
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}
