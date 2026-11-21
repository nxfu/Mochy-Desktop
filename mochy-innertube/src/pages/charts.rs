//! `pages/ChartsPage.kt` (18 lines).

use crate::models::ytitem::YTItem;

/// `ChartsPage.kt:5-8`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartsPage {
    pub sections: Vec<ChartSection>,
    pub continuation: Option<String>,
}

/// `ChartsPage.kt:9-13`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartSection {
    pub title: String,
    pub items: Vec<YTItem>,
    pub chart_type: ChartType,
}

/// `ChartsPage.ChartType` (ChartsPage.kt:15-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartType {
    Trending,
    Top,
    Genre,
    NewReleases,
}
