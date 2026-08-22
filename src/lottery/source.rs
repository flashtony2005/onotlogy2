//! 彩票数据源与 CSV 解析边界。
//!
//! 数据源只负责把外部文件转换成结构化 LotteryDraw；数据库幂等写入和导入
//! 批次记录仍由上层导入协调器负责。

use super::{GameSpec, LotteryDraw, LotteryError, LotteryGame};
use csv::{ReaderBuilder, StringRecord};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub(crate) trait LotteryDataSource {
    fn load(&self, game: LotteryGame) -> Result<Vec<LotteryDraw>, LotteryError>;
    fn source_file(&self) -> &Path;
    fn source_hash(&self) -> Result<String, LotteryError>;
}

#[derive(Debug, Clone)]
pub(crate) struct LotteryDrawInput {
    pub(crate) issue: String,
    pub(crate) draw_date: String,
    pub(crate) red_numbers: Vec<u8>,
    pub(crate) blue_numbers: Vec<u8>,
}

impl LotteryDrawInput {
    fn into_draw(self, game: LotteryGame, source: &str) -> LotteryDraw {
        let Self {
            issue,
            draw_date,
            red_numbers,
            blue_numbers,
        } = self;
        LotteryDraw {
            id: format!("lottery:draw:{}:{}", game.code(), issue),
            game: game.code().into(),
            issue,
            draw_date,
            red_numbers,
            blue_numbers,
            source: source.into(),
        }
    }
}

pub(crate) struct CsvFileSource {
    path: PathBuf,
}

impl CsvFileSource {
    pub(crate) fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

impl LotteryDataSource for CsvFileSource {
    fn load(&self, game: LotteryGame) -> Result<Vec<LotteryDraw>, LotteryError> {
        let source = self.path.to_string_lossy();
        CsvParser::new(game)
            .parse_file(&self.path)?
            .into_iter()
            .map(|input| Ok(input.into_draw(game, &source)))
            .collect()
    }

    fn source_file(&self) -> &Path {
        &self.path
    }

    fn source_hash(&self) -> Result<String, LotteryError> {
        let bytes = std::fs::read(&self.path)
            .map_err(|error| LotteryError::Invalid(format!("读取 CSV 失败：{error}")))?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }
}

pub(crate) struct CsvParser {
    game: LotteryGame,
}

impl CsvParser {
    pub(crate) fn new(game: LotteryGame) -> Self {
        Self { game }
    }

    pub(crate) fn parse_file(&self, path: &Path) -> Result<Vec<LotteryDrawInput>, LotteryError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|error| LotteryError::Invalid(format!("读取 CSV 失败：{error}")))?;
        self.parse_str(&contents)
    }

    pub(crate) fn parse_str(&self, contents: &str) -> Result<Vec<LotteryDrawInput>, LotteryError> {
        let mut reader = ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(contents.as_bytes());
        let headers = reader.headers()?.clone();
        let indexes = HeaderIndexes::from_headers(&headers, self.game.spec())?;
        reader
            .records()
            .enumerate()
            .map(|(row_index, result)| {
                let record = result?;
                parse_record(self.game.spec(), &indexes, &record, row_index + 2)
            })
            .collect()
    }
}

struct HeaderIndexes {
    issue: usize,
    date: usize,
    red: Vec<usize>,
    blue: Vec<usize>,
}

impl HeaderIndexes {
    fn from_headers(headers: &StringRecord, spec: GameSpec) -> Result<Self, LotteryError> {
        let names: Vec<String> = headers.iter().map(normalize_header).collect();
        let find = |name: &str| names.iter().position(|candidate| candidate == name);
        let issue = find("期号").ok_or_else(|| LotteryError::Invalid("CSV 缺少 期号 列".into()))?;
        let date = spec
            .date_names
            .iter()
            .find_map(|name| find(name))
            .ok_or_else(|| LotteryError::Invalid("CSV 缺少 日期/开奖日期 列".into()))?;
        let red = (1..=spec.red_count)
            .map(|i| {
                let name = format!("{}{}", spec.red_prefix, i);
                find(&name).ok_or_else(|| LotteryError::Invalid(format!("CSV 缺少 {name} 列")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let blue = (1..=spec.blue_count)
            .map(|i| {
                let name = if spec.blue_count == 1 && spec.blue_prefix == "蓝球" {
                    spec.blue_prefix.to_string()
                } else {
                    format!("{}{}", spec.blue_prefix, i)
                };
                find(&name).ok_or_else(|| LotteryError::Invalid(format!("CSV 缺少 {name} 列")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            issue,
            date,
            red,
            blue,
        })
    }
}

fn parse_record(
    spec: GameSpec,
    indexes: &HeaderIndexes,
    record: &StringRecord,
    row: usize,
) -> Result<LotteryDrawInput, LotteryError> {
    let issue = required_cell(record, indexes.issue, row, "期号")?;
    let draw_date = required_cell(record, indexes.date, row, "开奖日期")?;
    let red_numbers = indexes
        .red
        .iter()
        .map(|index| {
            parse_number(
                required_cell(record, *index, row, "红/前区号码")?,
                spec.red_max,
                row,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let blue_numbers = indexes
        .blue
        .iter()
        .map(|index| {
            parse_number(
                required_cell(record, *index, row, "蓝/后区号码")?,
                spec.blue_max,
                row,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_unique_sorted(&red_numbers, "红/前区", row)?;
    validate_unique_sorted(&blue_numbers, "蓝/后区", row)?;
    let mut red_numbers = red_numbers;
    let mut blue_numbers = blue_numbers;
    red_numbers.sort_unstable();
    blue_numbers.sort_unstable();
    Ok(LotteryDrawInput {
        issue,
        draw_date,
        red_numbers,
        blue_numbers,
    })
}

fn normalize_header(value: &str) -> String {
    value.chars().filter(|c| !c.is_whitespace()).collect()
}

fn required_cell(
    record: &StringRecord,
    index: usize,
    row: usize,
    label: &str,
) -> Result<String, LotteryError> {
    let value = record.get(index).unwrap_or("").trim();
    if value.is_empty() {
        return Err(LotteryError::Invalid(format!("第 {row} 行 {label} 为空")));
    }
    Ok(value.into())
}

fn parse_number(value: String, max: u8, row: usize) -> Result<u8, LotteryError> {
    let number = value
        .parse::<u8>()
        .map_err(|_| LotteryError::Invalid(format!("第 {row} 行号码无效：{value}")))?;
    if number == 0 || number > max {
        return Err(LotteryError::Invalid(format!(
            "第 {row} 行号码超出范围：{number}（1-{max}）"
        )));
    }
    Ok(number)
}

fn validate_unique_sorted(numbers: &[u8], label: &str, row: usize) -> Result<(), LotteryError> {
    if numbers.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(LotteryError::Invalid(format!(
            "第 {row} 行 {label} 存在重复号码"
        )));
    }
    Ok(())
}
