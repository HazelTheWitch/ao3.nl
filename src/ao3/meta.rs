use std::{string::FromUtf8Error, env};

use lazy_static::lazy_static;
use minify_html::{Cfg, minify};
use scraper::{Selector, Html, ElementRef};
use serde::Serialize;
use thiserror::Error;
use askama::Template;

use itertools::Itertools;

use nom::{
    IResult, bytes, combinator::{opt, map_res, recognize}, character::complete::digit1
};

lazy_static! {
    static ref WORK: Selector = Selector::parse(".work").unwrap();
    static ref WORK_HEADER: Selector = Selector::parse("div.header.module").unwrap();
    static ref TITLE: Selector = Selector::parse("h4>a:nth-child(1)").unwrap();
    static ref AUTHOR: Selector = Selector::parse("h4>a:nth-child(2)").unwrap();
    static ref FANDOMS: Selector = Selector::parse(".fandoms").unwrap();
    static ref DATE: Selector = Selector::parse(".datetime").unwrap();
    static ref TAG: Selector = Selector::parse(".tag").unwrap();
    static ref WARNINGS: Selector = Selector::parse(".warnings>strong>a").unwrap();
    static ref RELATIONSHIPS: Selector = Selector::parse(".relationships>a").unwrap();
    static ref CHARACTERS: Selector = Selector::parse(".characters>a").unwrap();
    static ref TAGS: Selector = Selector::parse(".freeforms>a").unwrap();
    static ref STATS: Selector = Selector::parse("dl.stats").unwrap();
    static ref LANGUAGE: Selector = Selector::parse("dd.language").unwrap();
    static ref WORDS: Selector = Selector::parse("dd.words").unwrap();
    static ref CHAPTERS: Selector = Selector::parse("dd.chapters").unwrap();
    static ref KUDOS: Selector = Selector::parse("dd.kudos>a").unwrap();
    static ref HITS: Selector = Selector::parse("dd.hits").unwrap();
    static ref SUMMARY: Selector = Selector::parse("blockquote.summary").unwrap();
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkMetadata {
    pub id: u64,
    pub title: String,
    pub author: String,
    pub published_date: String,
    pub fandoms: Vec<String>,
    pub warnings: Vec<String>,
    pub relationships: Vec<String>,
    pub characters: Vec<String>,
    pub tags: Vec<String>,
    pub language: Option<String>,
    pub words: u64,
    pub chapter: u16,
    pub total_chapters: Option<u16>,
    pub kudos: u32,
    pub hits: u64,
}

fn join_quoted(strings: Vec<String>) -> String {
    strings.into_iter()
        .map(|s| format!(r#""{}""#, s))
        .intersperse_with(|| String::from(", "))
        .collect()
}

#[derive(Debug, Serialize, Template)]
#[template(path = "work.html")]
pub struct WorkTemplate {
    pub id: u64,
    pub title: String,
    pub author: String,
    pub words: u64,
    pub chapters: String,
    pub date: String,
    pub description: String,
    pub host: String,
}

impl Into<WorkTemplate> for WorkMetadata {
    fn into(self) -> WorkTemplate {
        WorkTemplate {
            id: self.id,
            title: self.title,
            author: self.author,
            words: self.words,
            chapters: format!("{} / {}", self.chapter, self.total_chapters
                .map(|c| c.to_string())
                .unwrap_or_else(|| String::from("?"))),
            date: self.published_date,
            description: {
                let warnings = join_quoted(self.warnings);
                let characters = join_quoted(self.characters);
                let tags = join_quoted(self.tags);

                format!("{}\n{}\n{}", warnings, characters, tags)
            },
            host: env::var("HOST").unwrap_or_else(|_| String::from("http://localhost:3000")),
        }
    }
}

impl WorkTemplate {
    pub fn render_html(&self) -> Result<String, WorkError> {
        let html = self.render()?;

        let mut cfg = Cfg::new();
        cfg.do_not_minify_doctype = true;
        cfg.ensure_spec_compliant_unquoted_attribute_values = true;
        cfg.keep_spaces_between_attributes = true;

        let minified = minify(html.as_bytes(), &cfg);

        Ok(String::from_utf8(minified)?)
    }
}

#[derive(Debug, Error)]
pub enum WorkError {
    #[error("could not find the work information")]
    WorkError,
    #[error("could not parse work information")]
    ParsingError,
    #[error("could not request the work")]
    RequestError(#[from] reqwest::Error),
    #[error("error filling the template")]
    TemplatingError(#[from] askama::Error),
    #[error("minifying error")]
    Minify(#[from] FromUtf8Error),
}


fn chapter(input: &str) -> IResult<&str, u16> {
    map_res(recognize(digit1), str::parse)(input)
}

fn chapters(input: &str) -> IResult<&str, (u16, Option<u16>)> {
    let (input, chapter_value) = chapter(input)?;
    let (input, _) = bytes::complete::tag("/")(input)?;
    let (input, total_chapters) = opt(chapter)(input)?;

    Ok((input, (chapter_value, total_chapters)))
}

impl TryFrom<(u64, ElementRef<'_>)> for WorkMetadata {
    type Error = WorkError;

    fn try_from((id, work): (u64, ElementRef)) -> Result<Self, Self::Error> {
        let header = work.select(&WORK_HEADER).next().ok_or(WorkError::ParsingError)?;

        let title = header.select(&TITLE).next().ok_or(WorkError::ParsingError)?.inner_html();
        let author = header.select(&AUTHOR).next().ok_or(WorkError::ParsingError)?.inner_html();
        let fandoms = header.select(&FANDOMS).flat_map(|e|
            Some(e.select(&TAG)
                .next()?
                .inner_html())
        ).collect::<Vec<String>>();
        let date = header.select(&DATE).next().ok_or(WorkError::ParsingError)?.inner_html();

        let warnings = work.select(&WARNINGS).map(|e| e.inner_html()).collect::<Vec<String>>();
        let relationships = work.select(&RELATIONSHIPS).map(|e| e.inner_html()).collect::<Vec<String>>();
        let characters = work.select(&CHARACTERS).map(|e| e.inner_html()).collect::<Vec<String>>();
        let tags = work.select(&TAGS).map(|e| e.inner_html()).collect::<Vec<String>>();

        let stats = work.select(&STATS).next().ok_or(WorkError::ParsingError)?;

        let language = stats.select(&LANGUAGE).next().map(|e| e.inner_html());
        let words = stats.select(&WORDS).next().ok_or(WorkError::ParsingError)?.inner_html().replace(",", "").parse::<u64>().ok().ok_or(WorkError::ParsingError)?;
        let chapters_string = stats.select(&CHAPTERS).next().ok_or(WorkError::ParsingError)?.inner_html();


        let (chapter_value, total_chapters) = match chapters(&chapters_string) {
            Ok(("", (chapter, total))) => (chapter, total),
            _ => return Err(WorkError::ParsingError),
        };

        let kudos = stats.select(&KUDOS).next().ok_or(WorkError::ParsingError)?.inner_html().replace(",", "").parse::<u32>().ok().ok_or(WorkError::ParsingError)?;
        let hits = stats.select(&HITS).next().ok_or(WorkError::ParsingError)?.inner_html().replace(",", "").parse::<u64>().ok().ok_or(WorkError::ParsingError)?;

        Ok(WorkMetadata {
            id,
            title,
            author,
            published_date: date,
            fandoms,
            warnings,
            relationships,
            characters,
            tags,
            language,
            words,
            chapter: chapter_value,
            total_chapters,
            kudos,
            hits,
        })
    }
}

impl WorkMetadata {
    pub async fn work(id: u64) -> Result<Self, WorkError> {
        let url = format!("https://archiveofourown.org/works/{}", id);

        let html = reqwest::get(url)
            .await?
            .text()
            .await?;

        let html = Html::parse_document(&html);

        Ok((id, html.select(&WORK).next().ok_or(WorkError::WorkError)?).try_into()?)
    }
}
