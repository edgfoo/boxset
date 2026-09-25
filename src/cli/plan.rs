//! The section printed before a run: each source, what ffprobe found in it,
//! and what the run will make from it.

use std::path::Path;

use boxset::config::Codec;
use boxset::plan::Plan;
use boxset::sources::Probe;
use boxset::task::TaskKind;

use super::style::{bold, bold_dim, dim, dim_gray, icon, pad, section, visible_len, yellow};
use super::units::{directory, duration, filename, size};

const OVERWRITE_MARK: &str = "ˣ";

/// One source and every target planned from it
pub struct SourceBlock {
    probe: Probe,
    targets: Vec<TargetOutputs>,
}

struct TargetOutputs {
    /// One row per width, each holding that width's renditions and poster.
    rows: Vec<Row>,
    /// Subtitles are per target rather than per width, so they sit below.
    subtitles: Option<Cell>,
}

struct Row {
    width: u32,
    /// Indexed by `column_of`: one slot per codec, then the poster. A width
    /// that skips a codec leaves a hole rather than shifting the row.
    cells: Vec<Option<Cell>>,
}

#[derive(Clone)]
struct Cell {
    icon: String,
    name: String,
    exists: bool,
}

const COLUMNS: usize = 5;
const POSTER_COLUMN: usize = 4;

/// `None` for subtitles, which aren't in the grid.
fn column_of(kind: TaskKind) -> Option<(u32, usize)> {
    match kind {
        TaskKind::Rendition { width, codec } => {
            let column = match codec {
                Codec::H264 => 0,
                Codec::H265 => 1,
                Codec::Vp9 => 2,
                Codec::Av1 => 3,
            };
            Some((width, column))
        }
        TaskKind::Poster { width } => Some((width, POSTER_COLUMN)),
        TaskKind::Subtitles => None,
    }
}

/// Groups the plan by source, then by target
pub fn group(plan: &Plan) -> Vec<SourceBlock> {
    let mut blocks: Vec<SourceBlock> = Vec::new();
    let mut seen: Vec<(usize, usize, usize)> = Vec::new();

    for task in &plan.tasks {
        let block = match blocks.iter().position(|b| b.probe.src == task.probe.src) {
            Some(index) => index,
            None => {
                blocks.push(SourceBlock {
                    probe: (*task.probe).clone(),
                    targets: Vec::new(),
                });
                blocks.len() - 1
            }
        };

        // `seen` maps a plan target to its slot in this block, which is not
        // the same number: a block holds only the targets sharing its source.
        let slot = match seen
            .iter()
            .position(|&(b, t, _)| b == block && t == task.id.target)
        {
            Some(index) => seen[index].2,
            None => {
                blocks[block].targets.push(TargetOutputs {
                    rows: Vec::new(),
                    subtitles: None,
                });
                let slot = blocks[block].targets.len() - 1;
                seen.push((block, task.id.target, slot));
                slot
            }
        };
        let target = &mut blocks[block].targets[slot];

        let cell = Cell {
            icon: icon(task.id.kind),
            name: filename(&task.output_path),
            exists: task.exists,
        };

        let Some((width, column)) = column_of(task.id.kind) else {
            target.subtitles = Some(cell);
            continue;
        };
        let row = match target.rows.iter().position(|r| r.width == width) {
            Some(index) => &mut target.rows[index],
            None => {
                target.rows.push(Row {
                    width,
                    cells: vec![None; COLUMNS],
                });
                target.rows.last_mut().expect("just pushed")
            }
        };
        row.cells[column] = Some(cell);
    }

    blocks
}

/// Max width of complete source and output lines. If exceeded, outputs are
/// printed one-per-line.
const MAX_WIDTH: usize = 120;
const GUTTER: usize = 6;

/// At or below this many outputs, stack the outputs.
/// Each source block already occupies 3 lines with its name and probe data,
/// better to fill these rows with outputs if we only have a few.
const STACK_BELOW: usize = 4;

pub fn print_plan(blocks: &[SourceBlock], out_dir: &Path, targets: usize, outputs: usize) {
    section("Plan");

    println!(
        "  {} {} for {} {} will be written to {}",
        bold(&outputs.to_string()),
        plural(outputs, "output"),
        bold(&targets.to_string()),
        plural(targets, "target"),
        bold(&directory(out_dir))
    );

    let lead = blocks
        .iter()
        .flat_map(source_lines)
        .map(|line| visible_len(&line))
        .max()
        .unwrap_or(0);

    let is_stacked = blocks
        .iter()
        .flat_map(|block| &block.targets)
        .any(|target| {
            output_count(target) <= STACK_BELOW || lead + GUTTER + grid_width(target) > MAX_WIDTH
        });

    println!();
    println!(
        "  {}{}",
        pad(&dim_gray("Source"), lead + GUTTER),
        dim_gray("Outputs")
    );
    println!();

    for block in blocks {
        print_block(block, lead, is_stacked);
    }

    let overwritten = overwritten_count(blocks);
    if overwritten > 0 {
        let outputs = plural(overwritten, "output");
        println!(
            "  {} {} {}{}",
            bold_dim(&overwritten.to_string()),
            dim(&format!("existing {outputs} will be")),
            bold_dim("overwritten"),
            yellow(OVERWRITE_MARK),
        );
    }
}

fn source_lines(block: &SourceBlock) -> Vec<String> {
    let probe = &block.probe;

    let mut lines = vec![
        filename(&probe.src),
        dim(&format!("{}x{}", probe.width, probe.height)),
        dim(&format!(
            "{} {}",
            duration(probe.duration_secs),
            size(Some(probe.size_bytes))
        )),
    ];

    if !probe.has_audio {
        lines.push(dim("no audio"));
    }

    lines
}

fn print_block(block: &SourceBlock, lead: usize, stacked: bool) {
    let left = source_lines(block);
    let mut right: Vec<String> = Vec::new();

    for (index, target) in block.targets.iter().enumerate() {
        // For multiple targets with the same source, group all of their outputs
        if index > 0 {
            right.push(String::new());
        }
        right.extend(target_lines(target, stacked));
    }

    for index in 0..left.len().max(right.len()) {
        let source = left.get(index).cloned().unwrap_or_default();
        let outputs = right.get(index).cloned().unwrap_or_default();
        match outputs.is_empty() {
            true => println!("  {source}"),
            false => println!("  {}{}", pad(&source, lead + GUTTER), outputs),
        }
    }

    println!();
}

/// The widest row the grid would print.
fn grid_width(target: &TargetOutputs) -> usize {
    let used = used_columns(target);
    let widths = column_widths(target, &used);

    let Some((last, rest)) = widths.split_last() else {
        return 0;
    };

    let trailing = target
        .rows
        .iter()
        .filter_map(|row| row.cells[*used.last().expect("non-empty")].as_ref())
        .map(|cell| visible_len(&cell_text(cell)))
        .max()
        .unwrap_or(*last);

    rest.iter().sum::<usize>() + trailing + widths.len() - 1
}

fn output_count(target: &TargetOutputs) -> usize {
    target
        .rows
        .iter()
        .map(|row| row.cells.iter().flatten().count())
        .sum()
}

fn used_columns(target: &TargetOutputs) -> Vec<usize> {
    (0..COLUMNS)
        .filter(|&column| target.rows.iter().any(|r| r.cells[column].is_some()))
        .collect()
}

/// An icon, a space, the longest name in the column, and room for the
/// overwrite marker whether or not a given cell carries one.
fn column_widths(target: &TargetOutputs, used: &[usize]) -> Vec<usize> {
    used.iter()
        .map(|&column| {
            target
                .rows
                .iter()
                .filter_map(|r| r.cells[column].as_ref())
                .map(|c| c.name.chars().count())
                .max()
                .unwrap_or(0)
                + 3
        })
        .collect()
}

/// Prine the grid's rows as text, widths down and codecs across, with subtitles
/// last. A codec no width uses is dropped rather than printed as blanks.
/// `stacked` gives up the grid and prints one output per line.
fn target_lines(target: &TargetOutputs, stacked: bool) -> Vec<String> {
    let used = used_columns(target);

    let mut lines: Vec<String> = match stacked {
        true => target
            .rows
            .iter()
            .flat_map(|row| used.iter().filter_map(|&column| row.cells[column].as_ref()))
            .map(cell_text)
            .collect(),
        false => {
            let widths = column_widths(target, &used);
            target
                .rows
                .iter()
                .map(|row| {
                    used.iter()
                        .zip(&widths)
                        .map(|(&column, &width)| match &row.cells[column] {
                            Some(cell) => pad(&cell_text(cell), width),
                            None => " ".repeat(width),
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                        .trim_end()
                        .to_string()
                })
                .collect()
        }
    };

    if let Some(subtitles) = &target.subtitles {
        lines.push(cell_text(subtitles));
    }

    lines
}

fn cell_text(cell: &Cell) -> String {
    format!("{} {}{}", cell.icon, cell.name, mark(cell.exists))
}

fn mark(exists: bool) -> String {
    match exists {
        true => yellow(OVERWRITE_MARK),
        false => String::new(),
    }
}

fn overwritten_count(blocks: &[SourceBlock]) -> usize {
    blocks
        .iter()
        .flat_map(|b| &b.targets)
        .map(|target| {
            let grid = target
                .rows
                .iter()
                .flat_map(|r| r.cells.iter().flatten())
                .filter(|c| c.exists)
                .count();
            let subs = target.subtitles.iter().filter(|c| c.exists).count();
            grid + subs
        })
        .sum()
}

fn plural(count: usize, word: &str) -> String {
    match count {
        1 => word.to_string(),
        _ => format!("{word}s"),
    }
}
