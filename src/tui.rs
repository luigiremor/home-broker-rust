use crate::{Order, OrderBook, Trade};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    style::{Color, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};
use std::io::{self, stdout};
use std::time::Duration;

pub struct Tui {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl Tui {
    pub fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Tui { terminal })
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        Ok(())
    }

    pub fn should_quit() -> io::Result<bool> {
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                return Ok(key.code == KeyCode::Char('q')
                    || key.code == KeyCode::Char('c')
                        && key.modifiers.contains(event::KeyModifiers::CONTROL));
            }
        }
        Ok(false)
    }

    pub fn draw(&mut self, orderbook: &OrderBook) -> io::Result<()> {
        let mean_price = {
            let asks = orderbook.get_asks();
            let bids = orderbook.get_bids();
            Self::calculate_mean_price(&asks, &bids)
        };

        self.terminal.draw(|frame| {
            let size = frame.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(10)])
                .split(size);

            let tables_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                ])
                .split(chunks[1]);

            let header = Paragraph::new(format!("Mean Price: ${:.2}", mean_price))
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL));
            frame.render_widget(header, chunks[0]);

            let buy_orders =
                Self::create_orders_table("Buy Orders", orderbook.get_bids(), Color::Green);
            frame.render_widget(buy_orders, tables_layout[0]);

            let sell_orders =
                Self::create_orders_table("Sell Orders", orderbook.get_asks(), Color::Red);
            frame.render_widget(sell_orders, tables_layout[1]);

            let matched_orders =
                Self::create_matched_table("Matched Orders", orderbook.get_matched_orders());
            frame.render_widget(matched_orders, tables_layout[2]);
        })?;
        Ok(())
    }

    fn create_orders_table<'a>(title: &'a str, orders: Vec<Order>, color: Color) -> Table<'a> {
        let header_cells = ["Price", "Quantity"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow)));
        let header = Row::new(header_cells);

        let rows = orders.iter().map(|order| {
            let cells = [
                format!("${:.2}", order.price as f64 / 10f64.powi(order.decimals)),
                order.remaining.to_string(),
            ];
            Row::new(cells).style(Style::default().fg(color))
        });

        let widths = [Constraint::Percentage(50), Constraint::Percentage(50)];

        Table::new(rows, widths)
            .header(header)
            .block(Block::default().title(title).borders(Borders::ALL))
    }

    fn create_matched_table<'a>(title: &'a str, matches: Vec<Trade>) -> Table<'a> {
        let header_cells = ["Price", "Quantity", "Type"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow)));
        let header = Row::new(header_cells);

        let rows = matches.iter().map(|trade| {
            let cells = [
                format!("${:.2}", trade.price as f64 / 100.0),
                trade.quantity.to_string(),
                "MATCH".to_string(),
            ];
            Row::new(cells).style(Style::default().fg(Color::Blue))
        });

        let widths = [
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Percentage(30),
        ];

        Table::new(rows, widths)
            .header(header)
            .block(Block::default().title(title).borders(Borders::ALL))
    }

    fn calculate_mean_price(asks: &[Order], bids: &[Order]) -> f64 {
        let total_orders = asks.len() + bids.len();
        if total_orders == 0 {
            return 0.0;
        }

        let total_price: f64 = asks
            .iter()
            .chain(bids.iter())
            .map(|order| order.price as f64 / 10f64.powi(order.decimals))
            .sum();

        total_price / total_orders as f64
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
    }
}
