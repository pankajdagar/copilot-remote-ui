use crate::error::{AppError, Result};

pub fn dimensions(cols: u16, rows: u16) -> Result<(u16, u16)> {
    if !(2..=1000).contains(&cols) || !(2..=500).contains(&rows) {
        return Err(AppError::InvalidInput(
            "Terminal dimensions must be between 2x2 and 1000x500".into(),
        ));
    }

    Ok((cols, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_actual_terminal_dimension_limits() {
        assert!(dimensions(2, 2).is_ok());
        assert!(dimensions(1000, 500).is_ok());
        assert!(dimensions(1, 30).is_err());
        assert!(dimensions(100, 1).is_err());
        assert!(dimensions(1001, 30).is_err());
        assert!(dimensions(100, 501).is_err());
    }
}
