use std::{string::FromUtf8Error, env, num::ParseIntError};

use lazy_static::lazy_static;
use minify_html::{Cfg, minify};
use scraper::{Selector, Html, ElementRef};
use serde::Serialize;
use thiserror::Error;
use askama::Template;

use nom::{
    IResult, bytes, combinator::{opt, map_res, recognize}, character::complete::digit1
};

use crate::EmbedRequest;

lazy_static! {
    static ref TAG: Selector = Selector::parse("a.tag").unwrap();

    static ref TITLE: Selector = Selector::parse("h2.title").unwrap();
    static ref AUTHOR: Selector = Selector::parse(r#"a[rel="author"]"#).unwrap();

    static ref META_BLOCK: Selector = Selector::parse("dl.work").unwrap();

    static ref RATING: Selector = Selector::parse("dd.rating").unwrap();
    static ref WARNING: Selector = Selector::parse("dd.warning").unwrap();
    static ref CATEGORY: Selector = Selector::parse("dd.category").unwrap();
    static ref FANDOMS: Selector = Selector::parse("dd.fandom").unwrap();
    static ref RELATIONSHIPS: Selector = Selector::parse("dd.relationship").unwrap();
    static ref CHARACTERS: Selector = Selector::parse("dd.character").unwrap();
    static ref FREEFORMS: Selector = Selector::parse("dd.freeform").unwrap();
    static ref LANGUAGE: Selector = Selector::parse("dd.language").unwrap();

    static ref STATS_BLOCK: Selector = Selector::parse("dl.stats").unwrap();
    
    static ref PUBLISHED_DATE: Selector = Selector::parse("dd.published").unwrap();
    static ref WORDS: Selector = Selector::parse("dd.words").unwrap();
    static ref CHAPTERS: Selector = Selector::parse("dd.chapters").unwrap();
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkMetadata {
    pub id: u64,
    pub redirect_url: String,
    pub title: String,
    pub author: String,
    pub author_url: String,
    pub published_date: String,
    pub fandoms: Vec<String>,
    pub warnings: Vec<String>,
    pub relationships: Vec<String>,
    pub characters: Vec<String>,
    pub tags: Vec<String>,
    pub words: u64,
    pub chapter: u16,
    pub total_chapters: Option<u16>,
}

fn join_quoted(strings: Vec<String>) -> String {
    strings.into_iter()
        .intersperse_with(|| String::from(", "))
        .collect()
}

#[derive(Debug, Serialize, Template)]
#[template(path = "work.html")]
pub struct WorkTemplate {
    pub id: u64,
    pub redirect_url: String,
    pub author_url: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub embed_url: String,
}

impl From<WorkMetadata> for WorkTemplate {
    fn from(work: WorkMetadata) -> Self {
        let embed_request = EmbedRequest {
            id: work.id,
            author: work.author.clone(),
            words: work.words,
            chapters: work.chapter,
            total_chapters: work.total_chapters.map(|c| c.to_string()).unwrap_or_else(|| String::from("?")),
            date: work.published_date,
        };

        let embed_url = format!(
            "{}/oembed?{}",
            env::var("HOST").unwrap_or_else(|_| String::from("http://localhost:3000")),
            serde_urlencoded::to_string(embed_request).unwrap(),
        );

        Self {
            id: work.id,
            redirect_url: work.redirect_url,
            title: work.title,
            author: work.author,
            author_url: work.author_url,
            description: {
                let warnings = join_quoted(work.warnings);
                let characters = join_quoted(work.characters);
                let tags = join_quoted(work.tags);

                format!("{}\n{}\n{}", warnings, characters, tags)
            },
            embed_url,
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
    #[error("could not parse int")]
    IntError(#[from] ParseIntError),
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

struct Tag {
    pub href: Option<String>,
    pub text: String,
}

fn get_tags(element: ElementRef) -> impl Iterator<Item = Tag> + '_ {
    element
        .select(&TAG)
        .map(|t| Tag {
            href: t
                .value()
                .attr("href")
                .map(String::from),
            text: t.inner_html()
        })
}

fn select_one<'a>(parent: &ElementRef<'a>, selector: &'a Selector) -> Result<ElementRef<'a>, WorkError> {
    parent.select(selector).next().ok_or(WorkError::WorkError)
}

impl WorkMetadata {
    pub async fn work(id: u64, redirect_url: &str) -> Result<Self, WorkError> {
        let url = format!("https://archiveofourown.org/works/{}?view_adult=true", id);

        let html = reqwest::get(url)
            .await?
            .text()
            .await?;

        let html = Html::parse_document(&html);

        let meta = html.select(&META_BLOCK).next().ok_or(WorkError::WorkError)?;
        let stats = meta.select(&STATS_BLOCK).next().ok_or(WorkError::WorkError)?;
        
        let title = html.select(&TITLE)
            .next()
            .ok_or(WorkError::WorkError)?
            .inner_html();

        let author_element = html.select(&AUTHOR)
            .next()
            .ok_or(WorkError::WorkError)?;

        let author = author_element.inner_html();
        let author_url = author_element
            .value()
            .attr("href")
            .ok_or(WorkError::WorkError)?
            .to_string();

        let rating = get_tags(select_one(&meta, &RATING)?)
            .next()
            .ok_or(WorkError::WorkError)?
            .text;

        let warnings = get_tags(select_one(&meta, &WARNING)?)
            .map(|t| t.text)
            .collect::<Vec<_>>();

        let category = get_tags(select_one(&meta, &CATEGORY)?)
            .next()
            .ok_or(WorkError::WorkError)?
            .text;

        let fandoms = get_tags(select_one(&meta, &FANDOMS)?)
            .map(|t| t.text)
            .collect::<Vec<_>>();

        let relationships = get_tags(select_one(&meta, &RELATIONSHIPS)?)
            .map(|t| t.text)
            .collect::<Vec<_>>();

        let characters = get_tags(select_one(&meta, &CHARACTERS)?)
            .map(|t| t.text)
            .collect::<Vec<_>>();

        let freeforms = get_tags(select_one(&meta, &FREEFORMS)?)
            .map(|t| t.text)
            .collect::<Vec<_>>();

        // let language = get_tags(select_one(&meta, &LANGUAGE)?)
        //     .next()
        //     .ok_or(WorkError::WorkError)?
        //     .text;


        let published_date = select_one(&stats, &PUBLISHED_DATE)?.inner_html();
        let words = select_one(&stats, &WORDS)?.inner_html().replace(",", "").parse::<u64>()?;
        let chapters_string = select_one(&stats, &CHAPTERS)?.inner_html();

        let (chapter, total_chapters) = match chapters(&chapters_string) {
            Ok(("", (chapter, total))) => (chapter, total),
            _ => return Err(WorkError::ParsingError),
        };

        Ok(Self {
            id,
            redirect_url: redirect_url.to_string(),
            title,
            author,
            author_url,
            published_date,
            fandoms,
            warnings,
            relationships,
            characters,
            tags: freeforms,
            words,
            chapter,
            total_chapters,
        })
    }
}
