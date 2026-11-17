//! End-to-end compiler tests: source text in, IR or diagnostics out.

use darm_compiler::{Compilation, MemoryFs, compile, compile_str};
use darm_ir::{self as ir, Action, BlockKind, ChapterKind, Expr};

const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0,
    3, 0, 0, 0, 2, 8, 2, 0, 0, 0, 0x12, 0x16, 0xf1, 0x4d, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];
const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 300"><rect width="400" height="300" fill="#234"/></svg>"##;
const VTT: &str = "WEBVTT\n\n00:00.000 --> 00:02.000\nHello\n";

fn mp4(width: u16, height: u16) -> Vec<u8> {
    fn bx(ty: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(ty);
        v.extend_from_slice(body);
        v
    }
    let mut mvhd = vec![0u8; 100];
    mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
    mvhd[16..20].copy_from_slice(&2500u32.to_be_bytes());
    let mut tkhd = vec![0u8; 84];
    tkhd[76..78].copy_from_slice(&width.to_be_bytes());
    tkhd[80..82].copy_from_slice(&height.to_be_bytes());
    let mut moov = bx(b"mvhd", &mvhd);
    moov.extend(bx(b"trak", &bx(b"tkhd", &tkhd)));
    let mut out = bx(b"ftyp", b"isom\0\0\x02\0isomiso2");
    out.extend(bx(b"moov", &moov));
    out.extend(bx(b"mdat", &[0u8; 32]));
    out
}

const HEAD: &str =
    "darm 1\n\nbook \"Test Book\":\n    author = \"A. Writer\"\n    language = \"en\"\n\n";

fn with_head(body: &str) -> String {
    format!("{HEAD}{body}")
}

fn media_fs(source: &str) -> MemoryFs {
    MemoryFs::new()
        .with("book.darml", source.as_bytes().to_vec())
        .with("art/pic.png", PNG.to_vec())
        .with("art/map.svg", SVG.as_bytes().to_vec())
        .with("media/clip.mp4", mp4(640, 360))
        .with("media/sound.m4a", mp4(0, 0))
        .with("media/clip.en.vtt", VTT.as_bytes().to_vec())
        .with("media/clip.es.vtt", VTT.as_bytes().to_vec())
}

fn ok(c: &Compilation) -> &ir::Book {
    assert!(!c.has_errors(), "unexpected errors:\n{}", c.render(false));
    c.book.as_ref().expect("book")
}

fn codes(c: &Compilation) -> Vec<&'static str> {
    c.diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.code)
        .collect()
}

/// Compile `body` after a valid header and return the error codes.
fn errs(body: &str) -> Vec<&'static str> {
    let c = compile(&media_fs(&with_head(body)), "book.darml");
    assert!(c.book.is_none() || !c.has_errors());
    codes(&c)
}

fn blocks(book: &ir::Book) -> &[ir::Block] {
    &book
        .chapters
        .iter()
        .find(|c| c.kind == ChapterKind::Chapter)
        .unwrap()
        .blocks
}

#[test]
fn minimal_book_gets_covers_and_front_matter() {
    let c = compile_str(&with_head(
        "chapter \"One\":\n    text \"Hello *world*.\"\n",
    ));
    let book = ok(&c);
    assert_eq!(book.ir_version, "1.0");
    assert_eq!(book.meta.title, "Test Book");
    assert_eq!(book.meta.authors, ["A. Writer"]);
    assert!(book.meta.id.starts_with("darm-"));
    assert!(book.meta.offline);
    let kinds: Vec<_> = book.chapters.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        [
            ChapterKind::Title,
            ChapterKind::Colophon,
            ChapterKind::Contents,
            ChapterKind::Chapter
        ]
    );
    // Cover policy: generated deterministically, with warnings.
    assert!(book.cover.front.generated && book.cover.back.generated);
    assert_eq!(book.cover.front.title, "Test Book");
    let warnings: Vec<_> = c.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(warnings, ["W0601", "W0602"]);
    assert_eq!(book.toc.len(), 1);
    assert_eq!(book.toc[0].number, Some(1));
    let BlockKind::Paragraph { spans, .. } = &blocks(book)[0].kind else {
        panic!()
    };
    assert_eq!(spans[1].text, "world");
    assert!(spans[1].italic);
    // Deterministic output.
    let again = compile_str(&with_head(
        "chapter \"One\":\n    text \"Hello *world*.\"\n",
    ));
    assert_eq!(book.to_json(), again.book.unwrap().to_json());
    ir::validate(book).unwrap();
}

#[test]
fn full_featured_book_lowers_to_expected_ir() {
    let src = r##"darm 1

book "The Lighthouse":
    id = "org.example.lighthouse"
    subtitle = "A test"
    author = ["Ana", "Ben"]
    language = "en-GB"
    edition = "First"
    published = "2026-11-16"
    description = "A description."
    credit "Editor", "Cy"

theme:
    paper = "#fff"
    accent = "#9C3D2A"
    dark_paper = "#111111"
    dark_ink = "#eeeeee"
    dark_accent = "#ff9977"
    drop_caps = true
    paragraph = spaced

assets:
    pic = image("art/pic.png", alt="A picture")
    map = image("art/map.svg", alt="A map")
    clip = video("media/clip.mp4", alt="A clip", poster="art/pic.png", captions=["media/clip.en.vtt", "media/clip.es.vtt"])
    sound = audio("media/sound.m4a", title="Sound")

state:
    courage = 1 in 0..5
    lit = false
    route = "shore" in ["shore", "cliff"]

cover:
    style = band
    color = "#7a2e22"
    image = pic

back_cover:
    tagline = "Tag"
    text "Blurb one."
    text "Blurb two."

component note(body, label="Note"):
    box style=bar, title=label:
        text body, style=small

chapter "Arrival", id=arrival:
    text "First paragraph."
    section "The quay", id=quay:
        image pic, caption="Fig. *1*", width=60%
        gallery pic, map, caption="Both"
        video clip, autoplay=true, muted=true
        audio sound
        note "Mind the **gap**."
        note "Other", label="Warning"
    quote "A coast.", by="Proverb"
    list ordered=true:
        item "One"
        item "Two"
    divider style=rule
    space 2
    break
    text """
        Roses are red
        violets are blue
        """, style=verse, align=center
    choice "Which way?", id=fork:
        option "Cliff":
            route = "cliff"
            courage += 1
            goto cliff, transition=fade
        option "Shore", when=lit, goto=arrival
    if route == "cliff" and courage >= 2:
        text "High."
    elif lit:
        text "Bright {courage}."
    else:
        text "Low."
    quiz "2 + 2?":
        answer "4", correct=true
        answer "5"
        explain "Because."
        on_correct:
            courage += 1
    button "Again":
        reset
        goto start

chapter "Cliff", id=cliff, when=route == "cliff", numbered=false:
    text "Up here.", animate=rise, delay=200ms, duration=1s
    page background="#102030", id=plate:
        image map, at=(0, 0), size=(100, 100), fit=cover
        text "Title", style=display, at=(10, 10), size=(80, 20), color="#ffffff"
        button "Back", goto=quay, at=(30, 80), size=(40, 10)
"##;
    let c = compile(&media_fs(src), "book.darml");
    let book = ok(&c);
    assert!(c.diagnostics.is_empty(), "{}", c.render(false));
    ir::validate(book).unwrap();

    assert_eq!(book.meta.authors, ["Ana", "Ben"]);
    assert_eq!(book.theme.paper, "#ffffff");
    assert_eq!(book.theme.accent, "#9c3d2a");
    assert_eq!(book.theme.dark.as_ref().unwrap().paper, "#111111");
    assert_eq!(book.cover.front.style, ir::CoverStyle::Band);
    assert_eq!(book.cover.front.image.as_deref(), Some("pic"));
    assert_eq!(book.cover.back.blurb.len(), 2);
    assert!(!book.cover.front.generated);

    // Resources: declared + implicit poster and caption tracks.
    let names: Vec<_> = book.resources.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "pic",
            "map",
            "clip",
            "clip__poster",
            "clip__cc_en",
            "clip__cc_es",
            "sound"
        ]
    );
    let clip = book.resource("clip").unwrap();
    assert_eq!(
        (clip.width, clip.height, clip.duration_ms),
        (Some(640), Some(360), Some(2500))
    );
    assert_eq!(clip.poster.as_deref(), Some("clip__poster"));
    assert_eq!(
        clip.captions
            .iter()
            .map(|t| t.lang.as_str())
            .collect::<Vec<_>>(),
        ["en", "es"]
    );
    assert_eq!(book.resource("map").unwrap().media_type, "image/svg+xml");
    assert_eq!(book.resource("sound").unwrap().media_type, "audio/mp4");
    assert_eq!(c.files.len(), 7);
    assert!(
        c.files
            .iter()
            .any(|f| f.entry == "res/clip.mp4" && f.source == "media/clip.mp4")
    );

    assert_eq!(book.state.len(), 3);
    assert_eq!(
        book.state[0].ty,
        ir::StateType::Int {
            initial: 1,
            min: 0,
            max: 5
        }
    );

    // Anchors and contents.
    for id in ["arrival", "quay", "fork", "cliff", "plate"] {
        assert!(book.anchors.contains_key(id), "missing anchor {id}");
    }
    let toc: Vec<_> = book
        .toc
        .iter()
        .map(|t| (t.title.as_str(), t.level, t.number, t.when.is_some()))
        .collect();
    assert_eq!(
        toc,
        [
            ("Arrival", 1, Some(1), false),
            ("The quay", 2, None, false),
            ("Cliff", 1, None, true)
        ]
    );

    let arrival = book.chapters.iter().find(|c| c.id == "arrival").unwrap();
    // drop_caps turns the first body paragraph into a dropcap.
    assert!(matches!(
        arrival.blocks[0].kind,
        BlockKind::Paragraph {
            style: ir::TextStyle::Dropcap,
            ..
        }
    ));
    let BlockKind::Group {
        blocks: section,
        when: None,
    } = &arrival.blocks[1].kind
    else {
        panic!()
    };
    assert!(matches!(
        &section[0].kind,
        BlockKind::Heading { level: 2, .. }
    ));
    assert_eq!(section[0].id, "quay");
    assert!(
        matches!(&section[1].kind, BlockKind::Image { width, .. } if (*width - 0.6).abs() < 1e-9)
    );
    assert!(matches!(
        &section[3].kind,
        BlockKind::Video {
            autoplay: true,
            muted: true,
            looped: false,
            ..
        }
    ));
    // Components expand in place with their parameters substituted.
    let BlockKind::Box {
        style: ir::BoxStyle::Bar,
        title: Some(t),
        blocks: inner,
    } = &section[5].kind
    else {
        panic!()
    };
    assert_eq!(t, "Note");
    let BlockKind::Paragraph {
        spans,
        style: ir::TextStyle::Small,
    } = &inner[0].kind
    else {
        panic!()
    };
    assert!(spans[1].bold && spans[1].text == "gap");
    assert!(matches!(&section[6].kind, BlockKind::Box { title: Some(t), .. } if t == "Warning"));

    // Verse keeps line breaks.
    let verse = arrival.blocks.iter().find_map(|b| match &b.kind {
        BlockKind::Paragraph {
            spans,
            style: ir::TextStyle::Verse,
        } => Some((spans, b.align)),
        _ => None,
    });
    let (spans, align) = verse.unwrap();
    assert!(spans[0].line_break && !spans[1].line_break);
    assert_eq!(align, Some(ir::Align::Center));

    // Choice: actions in order, `+=` desugared, goto with transition.
    let choice = arrival.blocks.iter().find(|b| b.id == "fork").unwrap();
    let BlockKind::Choice { options, .. } = &choice.kind else {
        panic!()
    };
    assert_eq!(options[0].actions.len(), 3);
    assert!(
        matches!(&options[0].actions[1], Action::Set { var, value: Expr::Bin { kind: ir::BinOp::Add, .. } } if var == "courage")
    );
    assert!(
        matches!(&options[0].actions[2], Action::Goto { target, transition: Some(ir::Transition::Fade) } if target == "cliff")
    );
    assert!(options[1].when.is_some());
    assert!(
        matches!(&options[1].actions[..], [Action::Goto { target, .. }] if target == "arrival")
    );

    // if / elif / else become mutually exclusive groups.
    let groups: Vec<_> = arrival
        .blocks
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::Group { when: Some(w), .. } => Some(w),
            _ => None,
        })
        .collect();
    assert_eq!(groups.len(), 3);
    let mut state = ir::expr::initial_state(book);
    let truth = |s: &ir::State| {
        groups
            .iter()
            .map(|g| ir::expr::holds(Some(g), s))
            .collect::<Vec<_>>()
    };
    assert_eq!(truth(&state), [false, false, true]);
    state.insert("lit".into(), ir::Value::Bool(true));
    assert_eq!(truth(&state), [false, true, false]);
    state.insert("route".into(), ir::Value::Str("cliff".into()));
    state.insert("courage".into(), ir::Value::Int(2));
    assert_eq!(truth(&state), [true, false, false]);

    // Fixed page.
    let cliff = book.chapters.iter().find(|c| c.id == "cliff").unwrap();
    assert!(cliff.when.is_some() && cliff.number.is_none());
    assert_eq!(
        cliff.blocks[0].anim,
        Some(ir::Anim {
            kind: ir::AnimKind::Rise,
            delay_ms: 200,
            duration_ms: 1000
        })
    );
    let BlockKind::Page {
        background: Some(bg),
        blocks: page,
    } = &cliff.blocks[1].kind
    else {
        panic!()
    };
    assert_eq!(bg, "#102030");
    assert_eq!(
        page[0].frame,
        Some(ir::Frame {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0
        })
    );
    assert!(matches!(&page[2].kind, BlockKind::Button { .. }));

    // The IR survives a JSON round trip unchanged.
    let json = book.to_json();
    let back = ir::Book::from_json(&json).unwrap();
    assert_eq!(&back, book);
}

#[test]
fn includes_are_spliced_in_order() {
    let fs = MemoryFs::new()
        .with("book.darml", with_head("include \"parts/one.darml\"\ninclude \"parts/two.darml\"\n"))
        .with("parts/one.darml", "darm 1\nassets:\n    pic = image(\"../art/pic.png\", alt=\"x\")\nchapter \"One\":\n    image pic\n")
        .with("parts/two.darml", "darm 1\nchapter \"Two\":\n    text \"b\"\n")
        .with("art/pic.png", PNG.to_vec());
    let c = compile(&fs, "book.darml");
    let book = ok(&c);
    let titles: Vec<_> = book
        .chapters
        .iter()
        .filter(|c| c.kind == ChapterKind::Chapter)
        .map(|c| c.title.as_str())
        .collect();
    assert_eq!(titles, ["One", "Two"]);
    // Asset paths are relative to the file that declares them.
    assert_eq!(c.files[0].source, "art/pic.png");

    let cyc = MemoryFs::new()
        .with("book.darml", with_head("include \"a.darml\"\n"))
        .with("a.darml", "darm 1\ninclude \"book.darml\"\n");
    assert_eq!(codes(&compile(&cyc, "book.darml")), ["E0501"]);
    assert_eq!(
        errs("include \"missing.darml\"\nchapter \"A\":\n    text \"x\"\n"),
        ["E0501"]
    );
    assert_eq!(
        errs("include \"../outside.darml\"\nchapter \"A\":\n    text \"x\"\n"),
        ["E0402"]
    );
    assert_eq!(
        errs("include \"art/pic.png\"\nchapter \"A\":\n    text \"x\"\n"),
        ["E0501"]
    );
}

#[test]
fn asset_errors() {
    let ch = "chapter \"A\":\n    text \"x\"\n";
    let a = |decl: &str| errs(&format!("assets:\n    {decl}\n{ch}"));
    assert_eq!(a("p = image(\"art/missing.png\", alt=\"x\")"), ["E0401"]);
    assert_eq!(a("p = image(\"../../etc/passwd\", alt=\"x\")"), ["E0402"]);
    assert_eq!(a("p = image(\"/etc/passwd\", alt=\"x\")"), ["E0402"]);
    assert_eq!(a("p = image(\"media/clip.mp4\", alt=\"x\")"), ["E0403"]);
    assert_eq!(a("p = video(\"media/sound.m4a\", alt=\"x\")"), ["E0403"]);
    assert_eq!(a("p = image(\"art/pic.png\")"), ["E0308"]);
    assert_eq!(a("p = picture(\"art/pic.png\", alt=\"x\")"), ["E0301"]);
    assert_eq!(a("p = \"art/pic.png\""), ["E0307"]);
    assert_eq!(
        a("p = image(\"art/pic.png\", alt=\"x\", colour=1)"),
        ["E0303"]
    );
    assert_eq!(
        a("p = image(\"art/pic.png\", alt=\"x\")\n    p = image(\"art/pic.png\", alt=\"y\")"),
        ["E0306"]
    );
    assert_eq!(
        a("p = video(url=\"https://cdn.example.org/a.mp4\", alt=\"x\")"),
        ["E0406"]
    );
    assert_eq!(
        a("p = video(\"media/clip.mp4\", alt=\"x\", captions=\"art/pic.png\")"),
        ["E0403"]
    );

    let unsafe_svg = media_fs(&with_head(&format!(
        "assets:\n    p = image(\"art/bad.svg\", alt=\"x\")\n{ch}"
    )))
    .with(
        "art/bad.svg",
        r#"<svg viewBox="0 0 1 1"><script>alert(1)</script></svg>"#
            .as_bytes()
            .to_vec(),
    );
    assert_eq!(codes(&compile(&unsafe_svg, "book.darml")), ["E0405"]);

    // Uses of a broken asset do not cascade.
    assert_eq!(
        errs(
            "assets:\n    p = image(\"art/missing.png\", alt=\"x\")\nchapter \"A\":\n    image p\n"
        ),
        ["E0401"]
    );
    assert_eq!(errs("chapter \"A\":\n    image nope\n"), ["E0304"]);
    assert_eq!(
        errs("assets:\n    p = image(\"art/pic.png\", alt=\"x\")\nchapter \"A\":\n    video p\n"),
        ["E0307"]
    );
    assert_eq!(
        errs("chapter \"A\":\n    image \"art/pic.png\"\n"),
        ["E0304"]
    );
}

#[test]
fn remote_assets_follow_the_network_policy() {
    let src = "darm 1\nbook \"T\":\n    author = \"A\"\n    language = \"en\"\n    offline = false\n    network = [\"cdn.example.org\"]\nassets:\n    v = video(url=\"https://cdn.example.org/a.webm\", alt=\"x\")\nchapter \"A\":\n    video v\n";
    let c = compile_str(src);
    let book = ok(&c);
    assert!(!book.meta.offline);
    assert_eq!(
        book.resources[0].source,
        ir::ResourceSource::Remote {
            url: "https://cdn.example.org/a.webm".into()
        }
    );
    assert_eq!(book.resources[0].media_type, "video/webm");
    assert!(c.files.is_empty());
    assert_eq!(
        codes(&compile_str(
            &src.replace("cdn.example.org/a", "evil.example.com/a")
        )),
        ["E0406"]
    );
    assert_eq!(
        codes(&compile_str(&src.replace("https://cdn", "http://cdn"))),
        ["E0406"]
    );
    assert_eq!(
        codes(&compile_str(&src.replace("    offline = false\n", ""))),
        ["E0406", "E0406"]
    );
}

#[test]
fn structural_errors() {
    assert_eq!(
        codes(&compile_str("darm 1\nchapter \"A\":\n    text \"x\"\n")),
        ["E0313"]
    );
    assert_eq!(codes(&compile_str(HEAD)), ["E0313"]);
    assert_eq!(
        codes(&compile_str(
            "darm 1\nbook \"T\":\n    author = \"A\"\nchapter \"A\":\n    text \"x\"\n"
        )),
        ["E0308"]
    );
    assert_eq!(
        codes(&compile_str(
            "darm 1\nbook \"T\":\n    author = \"A\"\n    language = \"english language\"\nchapter \"A\":\n    text \"x\"\n"
        )),
        ["E0312"]
    );
    assert_eq!(
        errs(
            "book \"Again\":\n    author = \"B\"\n    language = \"en\"\nchapter \"A\":\n    text \"x\"\n"
        ),
        ["E0306"]
    );
    assert_eq!(
        errs("text \"loose\"\nchapter \"A\":\n    text \"x\"\n"),
        ["E0302"]
    );
    assert_eq!(
        errs("chapter \"A\":\n    chapter \"B\":\n        text \"x\"\n"),
        ["E0302"]
    );
    assert_eq!(errs("chapter \"A\":\n    paragraph \"x\"\n"), ["E0301"]);
    assert_eq!(errs("chapter \"A\":\n    item \"x\"\n"), ["E0302"]);
    assert_eq!(errs("chapter \"A\":\n    goto start\n"), ["E0302"]);
    assert_eq!(
        errs("chapter \"A\":\n    text \"x\":\n        text \"y\"\n"),
        ["E0313"]
    );
    assert_eq!(errs("chapter \"A\"\n"), ["E0313"]);
    assert_eq!(
        errs("chapter \"A\", when=flag:\n    text \"x\"\nstate:\n    flag = false\n"),
        ["E0313"]
    );
    assert_eq!(errs("chapter \"A\":\n    x = 1\n"), ["E0302"]);
    assert_eq!(
        errs("frobnicate:\n    a = 1\nchapter \"A\":\n    text \"x\"\n"),
        ["E0301"]
    );
    assert_eq!(
        errs("theme:\n    papre = \"#ffffff\"\nchapter \"A\":\n    text \"x\"\n"),
        ["E0303"]
    );
    assert_eq!(
        errs("theme:\n    paper = \"beige\"\nchapter \"A\":\n    text \"x\"\n"),
        ["E0312"]
    );
    assert_eq!(
        errs("theme:\n    dark_paper = \"#000000\"\nchapter \"A\":\n    text \"x\"\n"),
        ["E0308"]
    );
    assert_eq!(
        errs("cover:\n    show_text = false\nchapter \"A\":\n    text \"x\"\n"),
        ["E0308"]
    );
}

#[test]
fn argument_errors() {
    let b = |line: &str| errs(&format!("chapter \"A\":\n    {line}\n"));
    assert_eq!(b("text \"x\", colour=\"#333333\""), ["E0303"]);
    assert_eq!(b("text \"x\", \"y\""), ["E0303"]);
    assert_eq!(b("text \"x\", style=lead, style=lead"), ["E0306"]);
    assert_eq!(b("text"), ["E0308"]);
    assert_eq!(b("text 42"), ["E0307"]);
    assert_eq!(b("text \"x\", style=shouty"), ["E0312"]);
    assert_eq!(b("text \"x\", color=\"red\""), ["E0312"]);
    assert_eq!(b("text \"x\", scale=99"), ["E0312"]);
    assert_eq!(b("text \"x\", delay=1s"), ["E0308"]);
    assert_eq!(b("text \"x\", animate=fade, duration=1"), ["E0307"]);
    assert_eq!(b("text \"x\", at=(1, 2), size=(3, 4)"), ["E0316"]);
    assert_eq!(b("text \"2 *3 = 6\""), ["E0314"]);
    assert_eq!(b("text \"see [here](nowhere)\""), ["E0305"]);
    assert_eq!(b("text \"{nope}\""), ["E0314"]);
    assert_eq!(b("space 0"), ["E0312"]);
    assert_eq!(b("button \"Go\""), ["E0308"]);
    assert_eq!(b("button \"Go\", goto=nowhere"), ["E0305"]);
    assert_eq!(b("button \"Go\", goto=_title"), ["E0305"]);
    assert_eq!(b("text \"x\", id=start"), ["E0317"]);
    assert_eq!(b("text \"x\", id=dup\n    text \"y\", id=dup"), ["E0306"]);
    assert_eq!(
        b("quiz \"Q\":\n        answer \"a\"\n        answer \"b\""),
        ["E0308"]
    );
    assert_eq!(
        b("quiz \"Q\":\n        answer \"a\", correct=true"),
        ["E0312"]
    );
    assert_eq!(b("choice \"Q\":\n        text \"x\""), ["E0302"]);
    assert_eq!(b("list:\n        text \"x\""), ["E0302"]);
    assert_eq!(b("gallery nope1, nope2"), ["E0304", "E0304"]);
}

#[test]
fn state_and_expression_errors() {
    let s = |decl: &str| {
        errs(&format!(
            "state:\n    {decl}\nchapter \"A\":\n    text \"x\"\n"
        ))
    };
    assert_eq!(s("n = 9 in 0..5"), ["E0312"]);
    assert_eq!(s("n = 0 in 5..0"), ["E0312"]);
    assert_eq!(s("n = \"a\""), ["E0308"]);
    assert_eq!(s("n = \"z\" in [\"a\", \"b\"]"), ["E0312"]);
    assert_eq!(s("n = 1.5"), ["E0307"]);
    assert_eq!(s("n = 0\n    n = 1"), ["E0306"]);
    assert_eq!(s("n = 0 in 0..9999999999"), ["E0315"]);

    let st = "state:\n    n = 0 in 0..5\n    flag = false\n    route = \"a\" in [\"a\", \"b\"]\n";
    let e = |body: &str| errs(&format!("{st}chapter \"A\":\n{body}"));
    assert_eq!(e("    if n:\n        text \"x\"\n"), ["E0311"]);
    assert_eq!(e("    if n > flag:\n        text \"x\"\n"), ["E0311"]);
    assert_eq!(e("    if flag == 1:\n        text \"x\"\n"), ["E0311"]);
    assert_eq!(e("    if unknown:\n        text \"x\"\n"), ["E0310"]);
    assert_eq!(e("    if route == \"c\":\n        text \"x\"\n"), ["E0312"]);
    assert_eq!(e("    if n > 1.5:\n        text \"x\"\n"), ["E0311"]);
    assert_eq!(e("    button \"B\":\n        flag = 1\n"), ["E0311"]);
    assert_eq!(e("    button \"B\":\n        flag += 1\n"), ["E0311"]);
    assert_eq!(e("    button \"B\":\n        route = \"zzz\"\n"), ["E0312"]);
    assert_eq!(e("    button \"B\":\n        nope = 1\n"), ["E0310"]);
    assert_eq!(e("    button \"B\":\n        text \"x\"\n"), ["E0302"]);
    assert_eq!(e("    button \"B\":\n        reset nope\n"), ["E0310"]);
    assert_eq!(
        e("    button \"B\":\n        if flag:\n            n = 1\n"),
        ["E0302"]
    );
    // A working action block with every action form.
    let good = compile_str(&with_head(&format!(
        "{st}chapter \"A\":\n    text \"{{n}} {{flag}} {{route}}\"\n    button \"B\":\n        n = n * 2 + 1\n        n -= 1\n        flag = not flag or n >= 3\n        route = \"b\"\n        reset n\n        goto end\n"
    )));
    let book = ok(&good);
    let BlockKind::Button { actions, .. } = &blocks(book)[1].kind else {
        panic!()
    };
    assert_eq!(actions.len(), 6);
    let mut state = ir::expr::initial_state(book);
    let fx = ir::expr::apply(book, actions, &mut state);
    assert_eq!(state["n"], ir::Value::Int(0));
    assert_eq!(state["flag"], ir::Value::Bool(true));
    assert_eq!(state["route"], ir::Value::Str("b".into()));
    assert_eq!(fx.goto.unwrap().0, "end");
}

#[test]
fn component_errors() {
    let c = |defs: &str, use_: &str| errs(&format!("{defs}\nchapter \"A\":\n    {use_}\n"));
    assert_eq!(c("component a():\n    a", "a"), ["E0309"]);
    assert_eq!(
        c("component a():\n    b\ncomponent b():\n    a", "a"),
        ["E0309"]
    );
    assert_eq!(c("component a(x):\n    text x", "a"), ["E0309"]);
    assert_eq!(
        c("component a(x):\n    text x", "a \"1\", \"2\""),
        ["E0309"]
    );
    assert_eq!(
        c("component a(x):\n    text x", "a \"1\", y=\"2\""),
        ["E0303"]
    );
    assert_eq!(c("component text():\n    space 1", "space 1"), ["E0317"]);
    assert_eq!(
        c(
            "component a():\n    space 1\ncomponent a():\n    space 2",
            "a"
        ),
        ["E0306"]
    );
    assert_eq!(
        c("component a(x=\"1\", y):\n    text x", "a y=\"2\""),
        ["E0309"]
    );
}

#[test]
fn fixed_layout_rules() {
    let head = "darm 1\nbook \"T\":\n    author = \"A\"\n    language = \"en\"\n    layout = fixed\n    page_size = (4, 3)\n";
    let good = compile_str(&format!(
        "{head}chapter \"A\":\n    page:\n        text \"x\", at=(0, 0), size=(50, 50)\n"
    ));
    let book = ok(&good);
    assert_eq!(book.meta.layout, ir::Layout::Fixed);
    assert!((book.meta.page_aspect - 4.0 / 3.0).abs() < 1e-9);
    assert_eq!(
        codes(&compile_str(&format!(
            "{head}chapter \"A\":\n    text \"x\"\n"
        ))),
        ["E0316"]
    );
    assert_eq!(
        errs("chapter \"A\":\n    page:\n        text \"x\"\n"),
        ["E0316"]
    );
    assert_eq!(
        errs("chapter \"A\":\n    page:\n        text \"x\", at=(60, 0), size=(50, 50)\n"),
        ["E0316"]
    );
    assert_eq!(
        errs("chapter \"A\":\n    page:\n        text \"x\", at=(0, 0)\n"),
        ["E0316"]
    );
    assert_eq!(
        errs(
            "chapter \"A\":\n    box:\n        page:\n            text \"x\", at=(0, 0), size=(1, 1)\n"
        ),
        ["E0316"]
    );
    assert_eq!(
        errs("chapter \"A\":\n    page:\n        break\n"),
        ["E0302"]
    );
    // Conditional content inside a page is allowed; the children carry frames.
    let cond = compile_str(&with_head(
        "state:\n    f = false\nchapter \"A\":\n    page:\n        if f:\n            text \"x\", at=(0, 0), size=(10, 10)\n        else:\n            text \"y\", at=(0, 0), size=(10, 10)\n",
    ));
    ok(&cond);
}

#[test]
fn warnings_do_not_block_the_build() {
    let c = compile(
        &media_fs(&with_head(
            "assets:\n    pic = image(\"art/pic.png\", alt=\"x\")\nstate:\n    n = 0\ncomponent c():\n    space 1\nchapter \"A\":\n    text \"x\"\nchapter \"Empty\":\n    if true:\n        space 1\n",
        )),
        "book.darml",
    );
    assert!(c.book.is_some());
    let mut w: Vec<_> = c.diagnostics.iter().map(|d| d.code).collect();
    w.sort();
    assert_eq!(w, ["W0601", "W0602", "W0603", "W0604", "W0604"]);
    assert_eq!(c.warning_count(), 5);
}

#[test]
fn diagnostics_render_with_position_and_help() {
    let c = compile(
        &media_fs(&with_head(
            "assets:\n    tide_chart = image(\"art/pic.png\", alt=\"x\")\nchapter \"A\":\n    image tide_chrt\n",
        )),
        "book.darml",
    );
    let text = c.render(false);
    assert!(
        text.contains("error[E0304]: unknown asset `tide_chrt`"),
        "{text}"
    );
    assert!(text.contains("--> book.darml:10:11"), "{text}");
    assert!(text.contains("did you mean `tide_chart`?"), "{text}");
    let json = c.to_json();
    let first = json
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "E0304")
        .unwrap();
    assert_eq!(first["line"], 10);
    assert_eq!(first["file"], "book.darml");
    assert_eq!(first["severity"], "error");
}

#[test]
fn syntax_errors_stop_before_semantic_analysis() {
    let c = compile_str(
        "darm 1\nbook \"T\"\n    author = \"A\"\nchapter \"A\":\n    text \"a\" \"b\"\n",
    );
    assert!(c.book.is_none());
    assert!(
        codes(&c)
            .iter()
            .all(|c| c.starts_with("E01") || c.starts_with("E02")),
        "{:?}",
        codes(&c)
    );
    let not_utf8 = MemoryFs::new().with("book.darml", vec![b'd', 0xff, 0xfe]);
    assert_eq!(codes(&compile(&not_utf8, "book.darml")), ["E0107"]);
    assert_eq!(codes(&compile(&MemoryFs::new(), "book.darml")), ["E0501"]);
}

#[test]
fn every_emitted_code_is_documented() {
    let sources = [
        include_str!("../src/lower.rs"),
        include_str!("../src/decl.rs"),
        include_str!("../src/lib.rs"),
        include_str!("../src/assets.rs"),
        include_str!("../../darm-lang/src/lexer.rs"),
        include_str!("../../darm-lang/src/parser.rs"),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for src in sources {
        let bytes = src.as_bytes();
        for i in 0..bytes.len().saturating_sub(6) {
            if bytes[i] == b'"'
                && (bytes[i + 1] == b'E' || bytes[i + 1] == b'W')
                && bytes[i + 6] == b'"'
                && bytes[i + 2..i + 6].iter().all(u8::is_ascii_digit)
            {
                seen.insert(&src[i + 1..i + 6]);
            }
        }
    }
    assert!(seen.len() > 40, "code scan found too little: {seen:?}");
    for code in &seen {
        assert!(
            darm_compiler::explain::lookup(code).is_some(),
            "{code} is emitted but missing from explain::CATALOGUE"
        );
    }
    // And the reference export is well-formed JSON with every element.
    let reference = darm_compiler::spec::reference_json();
    assert!(reference["elements"].as_array().unwrap().len() >= 30);
    assert!(darm_compiler::explain::markdown().contains("## E0304"));
}

#[test]
fn hostile_sources_never_panic() {
    let mut seed = 0x2545F4914F6CDD1Du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let base = include_str!("fixtures/kitchen_sink.darml");
    let alphabet: Vec<char> = "\n\n    \t\"\"\"'#:=,()[]{}*`\\%.-+<>!abcxyz019 é\u{0}"
        .chars()
        .collect();
    for _ in 0..1500 {
        let mut chars: Vec<char> = base.chars().collect();
        for _ in 0..(next() % 6 + 1) {
            let i = (next() as usize) % chars.len();
            match next() % 4 {
                0 => {
                    chars.remove(i);
                }
                1 => chars.insert(i, alphabet[(next() as usize) % alphabet.len()]),
                2 => chars[i] = alphabet[(next() as usize) % alphabet.len()],
                _ => {
                    let j = (next() as usize) % chars.len();
                    chars.swap(i, j);
                }
            }
        }
        let src: String = chars.into_iter().collect();
        let c = compile(&media_fs(&src), "book.darml");
        if let Some(book) = &c.book {
            ir::validate(book).expect("a successful compile always yields valid IR");
        }
        // Formatting must never panic either, and must be stable when it succeeds.
        if let Ok(f) = darm_lang::fmt::format_source(darm_lang::FileId(0), "t", &src) {
            assert_eq!(
                darm_lang::fmt::format_source(darm_lang::FileId(0), "t", &f).as_deref(),
                Ok(f.as_str())
            );
        }
    }
}
