use core::option::Option;
use core::option::Option::{None, Some};
use std::cmp::max;
use std::ops::Add;
use itertools::Itertools;
use tui::text::Span;
use std::time::{Duration, Instant};
use tui::style::Style;
use tui::symbols;
use tui::widgets::{Dataset, GraphType, Paragraph};

pub struct PlotData {
    pub display: String,
    pub data: Vec<(f64, f64)>,
    pub style: Style,
    buffer: f64,
    simple_graphics: bool,
    idx: f64,
}

impl PlotData {
    pub fn new(display: String, buffer: f64, style: Style, simple_graphics: bool) -> PlotData {
        PlotData {
            display,
            data: Vec::with_capacity(150),
            style,
            buffer,
            simple_graphics,
            idx: 0.0,
        }
    }

    pub fn update(&mut self, item: Option<Duration>) {
        let idx = self.idx;
        match item {
            Some(dur) => self.data.push((idx, dur.as_micros() as f64)),
            None => self.data.push((idx, f64::NAN)),
        }
        let earliest_timestamp = idx - self.buffer;
        let last_idx = self
            .data
            .iter()
            .enumerate()
            .filter(|(_, (timestamp, _))| *timestamp < earliest_timestamp)
            .map(|(idx, _)| idx)
            .last();
        if let Some(idx) = last_idx {
            self.data.drain(0..idx).for_each(drop)
        }
        self.idx += 1.0;
    }

    pub fn header_stats(&self) -> Vec<Paragraph> {
        let ping_header = Paragraph::new(self.display.clone()).style(self.style);
        let items: Vec<&f64> = self
            .data
            .iter()
            .filter(|(_, x)| !x.is_nan())
            .sorted_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(_, v)| v)
            .collect();
        if items.is_empty() {
            return vec![ping_header];
        }

        let min = **items.first().unwrap();
        let max = **items.last().unwrap();
        let avg = items.iter().fold(0f64, |sum, &item| sum + item) / items.len() as f64;
        let jtr = items.iter().enumerate().fold(0f64, |sum, (idx, &item)| {
            sum + (*items.get(idx + 1).unwrap_or(&item) - item).abs()
        }) / (items.len() - 1) as f64;

        let percentile_position = 0.95 * items.len() as f32;
        let rounded_position = percentile_position.round() as usize;
        let p95 = items.get(rounded_position).map(|i| **i).unwrap_or(0f64);

        // count timeouts
        let to = self.data.iter().filter(|(_, x)| x.is_nan()).count();

        vec![
            ping_header,
            Paragraph::new(format!("min {:?}", Duration::from_micros(min as u64)))
                .style(self.style),
            Paragraph::new(format!("max {:?}", Duration::from_micros(max as u64)))
                .style(self.style),
            Paragraph::new(format!("avg {:?}", Duration::from_micros(avg as u64)))
                .style(self.style),
            Paragraph::new(format!("jtr {:?}", Duration::from_micros(jtr as u64)))
                .style(self.style),
            Paragraph::new(format!("p95 {:?}", Duration::from_micros(p95 as u64)))
                .style(self.style),
            Paragraph::new(format!("t/o {:?}", to)).style(self.style),
        ]
    }

    
    
    pub fn y_axis_bounds(&self) -> [f64; 2] {
        // Find the Y axis bounds for our chart.
        // This is trickier than the x-axis. We iterate through all our PlotData structs
        // and find the min/max of all the values. Then we add a 10% buffer to them.
        let iter = self
            .data
            .iter()
            .map(|v| v.1);
        let min = iter.clone().fold(f64::INFINITY, |a, b| a.min(b));
        let max = iter.fold(0f64, |a, b| a.max(b));
        // Add a 10% buffer to the top and bottom
        let max_10_percent = (max * 10_f64) / 100_f64;
        let min_10_percent = (min * 10_f64) / 100_f64;
        [min - min_10_percent, max + max_10_percent]
    }

    
    pub fn x_axis_bounds(&self) -> [f64; 2] {
        [if self.idx - self.buffer > 0.0 { self.idx - self.buffer } else { 0.0 }, self.idx]
    }

    
    pub fn x_axis_labels(&self, bounds: [f64; 2]) -> Vec<Span> {
        return vec![
            Span::raw(format!("{:?}", bounds[0])),
            // Span::raw(format!("{:?}", midpoint.time())),
            Span::raw(format!("{:?}", bounds[1])),
        ];
    }

    pub fn y_axis_labels(&self, bounds: [f64; 2]) -> Vec<Span> {
        // Create 7 labels for our y axis, based on the y-axis bounds we computed above.
        let min = bounds[0];
        let max = bounds[1];

        let difference = max - min;
        let num_labels = 7;
        // Split difference into one chunk for each of the 7 labels
        let increment = Duration::from_micros((difference / num_labels as f64) as u64);
        let duration = Duration::from_micros(min as u64);

        (0..num_labels)
            .map(|i| Span::raw(format!("{:?}", duration.add(increment * i))))
            .collect()
    }
}

impl<'a> From<&'a PlotData> for Dataset<'a> {
    fn from(plot: &'a PlotData) -> Self {
        let slice = plot.data.as_slice();
        Dataset::default()
            .marker(if plot.simple_graphics {
                symbols::Marker::Dot
            } else {
                symbols::Marker::Braille
            })
            .style(plot.style)
            .graph_type(GraphType::Line)
            .data(slice)
    }
}
