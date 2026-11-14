use super::*;

fn ser(name: &str, color: Option<&str>, cats: &[&str], vals: &[f64]) -> String {
    let pts = |v: Vec<String>| v.iter().enumerate().map(|(i, t)| format!(r#"<c:pt idx="{i}"><c:v>{t}</c:v></c:pt>"#)).collect::<String>();
    let fill = color.map(|c| format!(r#"<c:spPr><a:solidFill><a:srgbClr val="{c}"/></a:solidFill></c:spPr>"#)).unwrap_or_default();
    format!(r#"<c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>{name}</c:v></c:pt></c:strCache></c:strRef></c:tx>{fill}<c:cat><c:strRef><c:strCache>{}</c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache>{}</c:numCache></c:numRef></c:val></c:ser>"#,
        pts(cats.iter().map(|c| c.to_string()).collect()), pts(vals.iter().map(|v| v.to_string()).collect()))
}

fn chart_xml(title: &str, group: &str) -> String {
    let title = if title.is_empty() { String::new() } else { format!(r#"<c:title><c:tx><c:rich><a:p><a:r><a:t>{title}</a:t></a:r></a:p></c:rich></c:tx></c:title>"#) };
    format!(r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart>{title}<c:plotArea><c:layout/>{group}<c:catAx/><c:valAx/></c:plotArea></c:chart></c:chartSpace>"#)
}

const AT: PctRect = PctRect::new(10.0, 20.0, 80.0, 60.0);

fn fills(objs: &[Obj], color: &str) -> Vec<PctRect> {
    objs.iter().filter(|o| matches!(o.kind, ObjKind::Shape { .. }) && o.look.fill.as_deref() == Some(color)).map(|o| o.rect).collect()
}

fn texts(objs: &[Obj]) -> Vec<String> {
    objs.iter().filter_map(|o| match &o.kind { ObjKind::Text { text, .. } => Some(text.clone()), _ => None }).collect()
}

#[test]
fn a_column_chart_is_read_and_drawn_as_bars() {
    let group = format!(r#"<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>{}{}</c:barChart>"#,
        ser("2025", None, &["Jan", "Feb", "Mar"], &[10.0, 20.0, 5.0]), ser("2026", Some("FF0000"), &["Jan", "Feb", "Mar"], &[40.0, 0.0, 12.5]));
    let c = read(&chart_xml("Giving", &group)).unwrap();
    assert_eq!((c.kind, c.stacked, c.title.as_str()), (Kind::Columns, false, "Giving"));
    assert_eq!(c.categories, ["Jan", "Feb", "Mar"]);
    assert_eq!(c.series[1], Series { name: "2026".into(), color: Some("#ff0000".into()), values: vec![40.0, 0.0, 12.5] });
    let objs = draw(&c, AT).unwrap();
    // The panel first, where the chart was; everything else inside it.
    assert_eq!((objs[0].rect, objs[0].look.fill.as_deref()), (AT, Some(PANEL)));
    assert!(objs.iter().all(|o| o.rect.x >= AT.x - 0.01 && o.rect.y >= AT.y - 0.01 && o.rect.x + o.rect.w <= AT.x + AT.w + 0.01 && o.rect.y + o.rect.h <= AT.y + AT.h + 0.01), "{objs:#?}");
    // Three blue bars (Office's first colour) and two red ones (the zero has none), side by side, from one floor.
    let (blue, red) = (fills(&objs, PALETTE[0]), fills(&objs, "#ff0000"));
    assert_eq!((blue.len(), red.len()), (3, 2));
    let floor = blue[0].y + blue[0].h;
    assert!(blue.iter().chain(&red).all(|b| (b.y + b.h - floor).abs() < 0.01));
    // Heights go with the values: 40 is the tallest, twice 20, eight times 5.
    assert!((red[0].h / blue[1].h - 2.0).abs() < 0.01 && (red[0].h / blue[2].h - 8.0).abs() < 0.01);
    assert!((red[0].x - (blue[0].x + blue[0].w)).abs() < 0.01, "2026 beside 2025");
    // The title, the names under the bars, a value over each, and a legend in the series' colours.
    let t = texts(&objs);
    for want in ["Giving", "Jan", "Feb", "Mar", "40", "12.5", "■ 2025   ■ 2026"] { assert!(t.iter().any(|x| x == want), "{want}: {t:?}") }
    let legend = objs.iter().find_map(|o| match &o.kind { ObjKind::Text { text, paras, .. } if text.starts_with('■') => Some(paras.clone()), _ => None }).unwrap();
    assert_eq!(legend[0].runs[3].color.as_deref(), Some("#ff0000"));
}

#[test]
fn stacked_bars_lines_and_pies() {
    // Stacked, lying down: one bar a category, the second series starts where the first ends.
    let group = format!(r#"<c:barChart><c:barDir val="bar"/><c:grouping val="stacked"/>{}{}</c:barChart>"#,
        ser("Adults", None, &["Sun", "Wed"], &[30.0, 10.0]), ser("Children", None, &["Sun", "Wed"], &[10.0, 5.0]));
    let c = read(&chart_xml("", &group)).unwrap();
    assert_eq!((c.kind, c.stacked), (Kind::Bars, true));
    let objs = draw(&c, AT).unwrap();
    let (a, b) = (fills(&objs, PALETTE[0]), fills(&objs, PALETTE[1]));
    assert!((b[0].x - (a[0].x + a[0].w)).abs() < 0.01 && (a[0].y - b[0].y).abs() < 0.01, "{a:?} {b:?}");
    assert!((a[0].w / b[0].w - 3.0).abs() < 0.01);

    // A line chart: one drawn line a series, its points left to right, the highest value at the top.
    let group = format!(r#"<c:lineChart><c:grouping val="standard"/>{}</c:lineChart>"#, ser("Attendance", None, &["Q1", "Q2", "Q3"], &[100.0, 200.0, 150.0]));
    let objs = draw(&read(&chart_xml("", &group)).unwrap(), AT).unwrap();
    let ObjKind::Path { points, color, .. } = &objs.iter().find(|o| matches!(o.kind, ObjKind::Path { .. })).unwrap().kind else { panic!() };
    assert_eq!((points.len(), color.as_str()), (3, PALETTE[0]));
    assert!(points[0].0 == 0.0 && points[2].0 == 1.0 && points[1].1 == 0.0 && points[0].1 == 0.5, "{points:?}");

    // A pie: one bar cut into the shares, and the shares named.
    let group = format!(r#"<c:pieChart>{}</c:pieChart>"#, ser("Budget", None, &["Missions", "Building", "Staff"], &[25.0, 25.0, 50.0]));
    let c = read(&chart_xml("Budget", &group)).unwrap();
    assert_eq!(c.kind, Kind::Pie);
    let objs = draw(&c, AT).unwrap();
    let cuts: Vec<PctRect> = (0..3).map(|i| fills(&objs, PALETTE[i])[0]).collect();
    assert!((cuts[2].w / cuts[0].w - 2.0).abs() < 0.01 && (cuts[1].x - (cuts[0].x + cuts[0].w)).abs() < 0.01);
    assert!(texts(&objs).iter().any(|t| t == "■ Missions 25%   ■ Building 25%   ■ Staff 50%"), "{:?}", texts(&objs));
}

#[test]
fn charts_that_cannot_be_drawn_are_left_alone() {
    // A kind that is not drawn, and a chart with no numbers.
    assert_eq!(read(&chart_xml("", r#"<c:scatterChart><c:ser/></c:scatterChart>"#)), None);
    assert_eq!(read(&chart_xml("", r#"<c:barChart><c:barDir val="col"/></c:barChart>"#)), None);
    assert_eq!(read("<c:chartSpace"), None);
    // Twelve months of one series fit, labels and all; three series of twelve do not keep their labels; and bars
    // too thin to tell apart are not drawn at all.
    let months: Vec<String> = (1..=12).map(|m| format!("M{m}")).collect();
    let cats: Vec<&str> = months.iter().map(String::as_str).collect();
    let vals: Vec<f64> = (1..=12).map(f64::from).collect();
    let one = read(&chart_xml("", &format!(r#"<c:barChart><c:barDir val="col"/>{}</c:barChart>"#, ser("A", None, &cats, &vals)))).unwrap();
    let objs = draw(&one, AT).unwrap();
    assert!(objs.len() <= 40 && texts(&objs).iter().any(|t| t == "12"), "{}", objs.len());
    let two = format!(r#"<c:barChart><c:barDir val="col"/>{}{}</c:barChart>"#, ser("A", None, &cats, &vals), ser("B", None, &cats, &vals));
    let objs = draw(&read(&chart_xml("", &two)).unwrap(), AT).unwrap();
    assert!(objs.len() <= 40 && !texts(&objs).iter().any(|t| t == "12"), "no room for value labels: {}", objs.len());
    assert_eq!(draw(&one, PctRect::new(0.0, 0.0, 20.0, 60.0)), None, "bars thinner than the editor keeps");
}
