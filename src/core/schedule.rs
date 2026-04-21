//! Schedule builder — generates cron expressions from human-friendly inputs
//! and computes next run times using croner.

use croner::Cron;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weekday {
    Sunday,
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
}

impl Weekday {
    pub fn cron_number(&self) -> u8 {
        match self {
            Self::Sunday => 0,
            Self::Monday => 1,
            Self::Tuesday => 2,
            Self::Wednesday => 3,
            Self::Thursday => 4,
            Self::Friday => 5,
            Self::Saturday => 6,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Sunday => "Sun",
            Self::Monday => "Mon",
            Self::Tuesday => "Tue",
            Self::Wednesday => "Wed",
            Self::Thursday => "Thu",
            Self::Friday => "Fri",
            Self::Saturday => "Sat",
        }
    }

    pub fn all() -> &'static [Weekday] {
        &[
            Self::Sunday,
            Self::Monday,
            Self::Tuesday,
            Self::Wednesday,
            Self::Thursday,
            Self::Friday,
            Self::Saturday,
        ]
    }
}

pub fn build_hourly(minute: u8) -> String {
    format!("{} * * * *", minute)
}

#[allow(dead_code)]
pub fn build_every_n_hours(n: u8, _start_hour: u8, start_minute: u8) -> String {
    format!("{} */{} * * *", start_minute, n)
}

pub fn build_daily(hour: u8, minute: u8) -> String {
    format!("{} {} * * *", minute, hour)
}

pub fn build_weekly(days: &[Weekday], hour: u8, minute: u8) -> String {
    let day_nums: Vec<String> = days.iter().map(|d| d.cron_number().to_string()).collect();
    format!("{} {} * * {}", minute, hour, day_nums.join(","))
}

pub fn build_monthly_by_day(day: u8, hour: u8, minute: u8) -> String {
    format!("{} {} {} * *", minute, hour, day)
}

pub fn build_yearly(month: u8, day: u8, hour: u8, minute: u8) -> String {
    format!("{} {} {} {} *", minute, hour, day, month)
}

#[allow(dead_code)]
pub fn validate_cron(expr: &str) -> Result<(), String> {
    Cron::new(expr)
        .parse()
        .map(|_| ())
        .map_err(|e| format!("Invalid cron expression: {}", e))
}

pub fn next_run(expr: &str) -> Option<i64> {
    let cron = Cron::new(expr).parse().ok()?;
    let next = cron.find_next_occurrence(&chrono::Utc::now(), false).ok()?;
    Some(next.timestamp_millis())
}

pub fn human_readable(expr: &str) -> String {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return expr.to_string();
    }
    if Cron::new(expr).parse().is_err() {
        return expr.to_string();
    }

    let (minute, hour, dom, month, dow) = (parts[0], parts[1], parts[2], parts[3], parts[4]);

    // Every N hours
    if hour.starts_with("*/") && dom == "*" && month == "*" && dow == "*" {
        let n = &hour[2..];
        return format!("Every {} hours at :{:0>2}", n, minute);
    }

    // Hourly
    if hour == "*" && dom == "*" && month == "*" && dow == "*" {
        return format!("Every hour at :{:0>2}", minute);
    }

    let time_str = format!("{}:{:0>2}", hour, minute);

    // Yearly
    if dom != "*" && month != "*" && dow == "*" {
        let month_name = match month {
            "1" => "Jan",
            "2" => "Feb",
            "3" => "Mar",
            "4" => "Apr",
            "5" => "May",
            "6" => "Jun",
            "7" => "Jul",
            "8" => "Aug",
            "9" => "Sep",
            "10" => "Oct",
            "11" => "Nov",
            "12" => "Dec",
            _ => month,
        };
        return format!("Every year on {} {} at {}", month_name, dom, time_str);
    }

    // Monthly
    if dom != "*" && month == "*" && dow == "*" {
        return format!("Every month on day {} at {}", dom, time_str);
    }

    // Weekly
    if dom == "*" && month == "*" && dow != "*" {
        let day_names: Vec<&str> = dow
            .split(',')
            .map(|d| match d {
                "0" => "Sun",
                "1" => "Mon",
                "2" => "Tue",
                "3" => "Wed",
                "4" => "Thu",
                "5" => "Fri",
                "6" => "Sat",
                _ => d,
            })
            .collect();
        return format!("Every {} at {}", day_names.join(", "), time_str);
    }

    // Daily
    if dom == "*" && month == "*" && dow == "*" {
        return format!("Every day at {}", time_str);
    }

    expr.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daily_schedule() {
        let cron = build_daily(9, 0);
        assert_eq!(cron, "0 9 * * *");
    }

    #[test]
    fn test_hourly_schedule() {
        let cron = build_hourly(30);
        assert_eq!(cron, "30 * * * *");
    }

    #[test]
    fn test_weekly_schedule_single_day() {
        let cron = build_weekly(&[Weekday::Monday], 8, 0);
        assert_eq!(cron, "0 8 * * 1");
    }

    #[test]
    fn test_weekly_schedule_multiple_days() {
        let cron = build_weekly(
            &[Weekday::Monday, Weekday::Wednesday, Weekday::Friday],
            9,
            30,
        );
        assert_eq!(cron, "30 9 * * 1,3,5");
    }

    #[test]
    fn test_monthly_schedule_by_day() {
        let cron = build_monthly_by_day(15, 10, 0);
        assert_eq!(cron, "0 10 15 * *");
    }

    #[test]
    fn test_yearly_schedule() {
        let cron = build_yearly(3, 15, 9, 0);
        assert_eq!(cron, "0 9 15 3 *");
    }

    #[test]
    fn test_next_run_returns_future_time() {
        let next = next_run("0 9 * * *");
        assert!(next.is_some());
    }

    #[test]
    fn test_next_run_invalid_cron_returns_none() {
        let next = next_run("not a cron expression");
        assert!(next.is_none());
    }

    #[test]
    fn test_validate_cron_valid() {
        assert!(validate_cron("0 9 * * *").is_ok());
        assert!(validate_cron("30 */2 * * 1-5").is_ok());
    }

    #[test]
    fn test_validate_cron_invalid() {
        assert!(validate_cron("not valid").is_err());
        assert!(validate_cron("").is_err());
    }

    #[test]
    fn test_human_readable_daily() {
        let desc = human_readable("0 9 * * *");
        assert!(desc.contains("9:00"));
    }

    #[test]
    fn test_human_readable_weekly() {
        let desc = human_readable("0 9 * * 1,3,5");
        assert!(desc.contains("9:00"));
    }

    #[test]
    fn test_human_readable_invalid_returns_raw() {
        let desc = human_readable("bad cron");
        assert_eq!(desc, "bad cron");
    }

    #[test]
    fn test_every_n_hours() {
        let cron = build_every_n_hours(2, 0, 0);
        assert_eq!(cron, "0 */2 * * *");
    }
}
