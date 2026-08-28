# Screenplay Creation Tool

Write a screenplay in plain text and get back a correctly formatted script.

The tool reads [Fountain](https://fountain.io), the plain-text screenplay
markup — plus [one addition of its own](#typing-dialogue-quickly), a way to
write a speech on a single line, which is expanded back into ordinary Fountain
whenever a `.fountain` file is saved. It lays the result out to the industry
standard: a six-inch text column inside
a one-and-a-half-inch left margin, 12 point Courier, 55 lines to the page, with
character cues, parentheticals and dialogue each in their own column. It handles
the things that make screenplay formatting fiddly — `(MORE)` and `(CONT'D)`
across a page break, scene headings that must not be stranded at the foot of a
page, dual dialogue in two columns, scene numbers in both margins.

It is a desktop application, built on a formatting engine that is a library in
its own right — so what you read on screen is what comes out of the file. It
also [reads a script aloud](#audio-drama): the **Audio Drama** menu turns a
story file into a mixed radio play in ElevenLabs voices, pitched to the age on
each line and treated to match how it is said.

## The application

```sh
cargo run --release                          # start with an empty screenplay
cargo run --release -- examples/sample.fountain
cargo run --release -- examples/sample-drama.xml   # straight to the Audio Drama tab
```

Three panes over one document: the scene outline on the left, the Fountain
editor in the middle, and the formatted script on the right, repaginating as you
type. The status bar carries the page, scene and word counts. <kbd>F1</kbd>
lists the markup as well as the shortcuts.

### Saving your work: `.sct`

<kbd>⌘S</kbd> saves a **`.sct` document** — the tool's own working format. It is
the Fountain source with a short header in front of it recording where you had
got to and what you know about the story, so reopening a draft puts the caret
back where you left it, the outline back on the scene you were reading, and
your notes back beside the script:

```text
SCT/1
caret: 1402
scene: 3
world: Ashfen stands on a salt lake.\nThe Guild rules by writ.
character: MAYA
role: Salt-runner. The younger sister.
want: To buy her brother out of his indenture
voice: Short sentences. Never says what she means.
---
Title: The Last Bus

INT. BUS SHELTER - NIGHT
...
```

It is plain text on purpose. It diffs, it greps, and if the header is ever lost
or damaged the file is still your screenplay — anything without the `SCT/1`
line is read as ordinary Fountain. Header values are one line each, so a note
that runs to several lines writes its breaks as `\n`; unknown keys are ignored,
so a file written by a later version still opens here.

Save as `.fountain` instead and you get ordinary Fountain with no header, for
handing to another tool. That drops the notes as well as the caret, and the
tool says so when it does it.

### Typing dialogue quickly

Fountain writes a speech over three lines. This tool understands a one-line form
as well:

```text
MAYA: Forty-one.
DEV (not looking up): You know what this is.
/transition:cut to
```

which is the same screenplay as:

```text
MAYA
Forty-one.

DEV
(not looking up)
You know what this is.

CUT TO:
```

A speech written this way ends at the end of its line — press Return and you are
writing action again — but a line that is itself another cue carries on the
exchange, so a fast back-and-forth needs no blank lines. Brackets before the
colon become a stage direction under the cue, except `(V.O.)`, `(O.S.)`,
`(O.C.)` and `(CONT'D)`, which every screenplay prints on the cue line and so
does this. The words have to be on the same line as the colon, which is what
keeps `FADE IN:` the transition it has always been.

**This part is not Fountain**, and the tool does not pretend otherwise.
`MAYA: Forty-one.` in a file handed to another program is a line of action, and
that program would be right. So the shorthand is understood on the way in and
written out in full on the way out: save as `.fountain` and what lands on disk
is the three-line form. The tool's own `.sct` file keeps what you typed.

### Characters and world

<kbd>⌘K</kbd> opens **Characters and world** — the things you know about the
story that the script never says out loud.

At the top is the world: the setting and its rules, in your own words. Under it
is one profile per character — role, age, what they want, how they talk, and
whatever else needs writing down. None of it is printed. All of it is saved in
the `.sct` file with the screenplay, so the notes and the draft cannot be
separated.

There is no separate Save: the notes are part of the screenplay document, so
<kbd>⌘S</kbd> saves them with it, and the window says as much along the bottom.

Characters are keyed by the cue name they speak under, so a profile and the
dialogue in the script are about the same person. Anybody who speaks but has no
profile yet can be added in one press, and each profile shows how much that
character is in the script already.

### Ideas from a local model

<kbd>⌘I</kbd> opens **Ideas**, which puts one question to a language model
running on your own machine:

- **What would they do?** — three things this character could do next.
- **What would they say?** — three lines they could say, in their voice.
- **What happens next?** — three directions the story could go, written as
  Fountain synopsis lines (`= like this`), which are notes to yourself and
  never print.

The world, the character notes and the page you are on are what the model is
told, which is the whole point: a model that has read your notes on the Guild
and on Maya can answer as Maya, and one that has not cannot.

The answer lands in a box you can edit. Nothing reaches the screenplay until
you press **Insert at the caret**.

This is optional in every sense. No request is made until you ask for one, and
if no model server is running the window says so — and, when Ollama is installed
but not started, offers a **Start Ollama** button rather than an error message
about sockets. If it is not installed at all there is a link to go and get it.
The rest of the tool carries on exactly as before either way. Two shapes of server are understood:

| | |
|---|---|
| **Ollama** | `/api/tags` and `/api/generate` |
| **Anything OpenAI-compatible** | llama.cpp's server, LM Studio, Jan: `/v1/models` and `/v1/chat/completions` |

The tool works out which is there by asking. By default it looks at
`http://127.0.0.1:11434`, where Ollama listens; `OPENWRITE_AI_URL` points it
somewhere else and `OPENWRITE_AI_MODEL` picks the model without going through
the window. There is no TLS and no cloud provider: `https://` addresses are
refused rather than quietly accepted, and if the server you point it at is not
on this machine the window says so in as many words, because that is the one
case where an unpublished screenplay leaves the computer it was written on.

Builds made with `--no-default-features` have none of this compiled in at all.

### Audio drama

The **Audio Drama** menu turns a story file into a radio play: every line read
by an [ElevenLabs](https://elevenlabs.io) voice, pitched and treated to match
what the line says about itself, placed in the stereo picture, and mixed into
one `.wav`.

A story is XML — a cast list and the dialogue:

```xml
<story model="1.0">
    <voices>
        <character id="1" name="ben" gender="male">VOICE_ID</character>
        <character id="2" name="faith" gender="female">VOICE_ID</character>
    </voices>
    <dialog>
        <character line="1" id="1" age="12" state="normal" pos="left">I don't remember how it happened</character>
        <character line="2" id="2" age="15" state="whisper" pos="right">What?</character>
    </dialog>
</story>
```

Between the `<character>` tags in `<dialog>` is the line itself. The attributes
say how it is said:

| | |
|---|---|
| `age` | Shifts the pitch. A twelve-year-old does not sound like the adult whose voice was hired to play them. |
| `state` | `normal`, `whisper`, `scared`, `shout`, `angry`, `sad`, `excited`, `tired`. |
| `pos` | `left`, `centre`, `right`. |

`<voices>` is where the ElevenLabs voice for each character goes. **If your file
has not got one, the tab makes it** out of whoever speaks, offers you a voice
for each of them, and **Save story** writes it back — so the casting is done
once and kept in the file. The same is true of anybody who speaks but was left
out of an existing cast list. `examples/sample-drama.xml` is a whole one to
start from, and opening a `.xml` file — from the menu, from the command line, or
through **Open With** — goes straight to the tab.

**What is done to the audio.** All of it is arithmetic on samples, on your
machine, with nothing installed:

- **Age** moves the pitch without moving the duration — the line still takes as
  long to say — by stretching the waveform in time and resampling it back.
  Twelve is about four semitones up; eighty is a little down and a little
  slower. The **Age shift** slider scales the lot, for when the voice you cast
  already sounds the right age and you do not want it applied twice.
- **`state="scared"`** is a tremble: a slow wobble in pitch and one in loudness,
  which is what a frightened voice actually does. `whisper` thins the voice,
  drops it, and lays breath over it shaped by the words. `shout` drives it until
  it starts to break up. Each state also steers ElevenLabs itself — a frightened
  line is generated with low stability and high style, a whispered one with the
  opposite.
- **`pos`** is a constant-power pan, so a voice does not get louder or quieter
  for having moved across the room. Not panned all the way: a voice hard against
  one ear sounds like a fault rather than like a room.

The tab lists every line with what is about to happen to it — *"ben, 12, normal,
+4.0 semitones, 2% faster, left"* — before anything is sent, and reports
anything odd about the file as notes rather than errors: an unknown `state` is
read plainly, a missing `<voices>` section is built, and a voice id typed inside
the tag rather than between the tags is taken anyway.

**What it costs.** One paid ElevenLabs request per line. The raw recording of
each line is kept beside the finished play, named after a fingerprint of the
three things ElevenLabs was told — the voice, the state and the words — so a
later run that finds all three unchanged reuses it. Changing an age, a `pos`, or
the age slider and re-recording therefore **costs nothing at all**: none of those
change what was said, only what is done to it afterwards. Naming a recording
after what is in it also means it survives editing, so adding a line at the top
of a scene does not re-bill you for every line underneath it. Recordings no line
asks for any more are swept up at the end of a run.

Your API key can go in the settings file, where the tab's **Remember** button
puts it. That file is plain text: anyone who can read it can spend the key.
Setting `OPENWRITE_ELEVENLABS_KEY` in the environment takes precedence over it
and is never written to disk, which is the way to use a key without leaving it
lying about. `OPENWRITE_ELEVENLABS_MODEL` chooses the voice model
(`eleven_multilingual_v2` by default).

Nothing is sent until you press **Record**, and a recording can be stopped
between lines. HTTPS is `curl`, as it is for the update check — there is no TLS
stack and no other dependency in any of this. Builds made with
`--no-default-features` have none of it compiled in.

Sound effects are not in this version. `model="1.2"` is read and its dialogue is
spoken, but a story that expects footsteps will not get them.

### Accessibility

The window publishes a real accessibility tree through
[AccessKit](https://accesskit.dev) — VoiceOver on macOS, UI Automation on
Windows, AT-SPI on Linux — so a screen reader sees named controls rather than
painted pixels. On top of that:

- **Every command has a keyboard shortcut**, and the shortcut table is the same
  data the help window is generated from, so the two cannot drift apart. Press
  <kbd>F1</kbd> for the list.
- **Panes are named and described.** The preview announces itself as "Formatted
  preview" and says how to read it; the editor explains what Fountain markup is.
- **A live region.** When the tool does something — saves, exports, jumps to a
  scene, finds seven matches — it says so in the status bar, which is marked as
  a polite live region so a screen reader announces it without stealing focus.
- **A loud focus ring.** Three pixels, in a colour that clears the contrast floor
  on both themes, so keyboard focus is never ambiguous.
- **Light and dark themes** that follow the operating system, both built from a
  palette where every colour that carries meaning measures at least 4.5:1
  against every surface it is drawn on. There are tests that assert this.
- **A high contrast mode** (<kbd>⇧⌘H</kbd>) — pure black and white, every
  boundary drawn.
- **Text scaling** from 70% to 300% (<kbd>⌘+</kbd> / <kbd>⌘-</kbd> / <kbd>⌘0</kbd>).
- **Colour never carries meaning alone.** A status message says what happened in
  words; the colour only agrees with it.

### Language

Every word the window says is looked up in a language file rather than written
into the program, so the editor can be translated by somebody who does not
program. **View → Language…** picks the language, or follows the computer's own
setting; the choice is remembered, and changing it redraws the whole window
without a restart.

A language is one plain text file:

```toml
code   = "fr"
name   = "Français"          # named in its own language, for the picker
plural = "french"

menu.file = "Fichier"
status.saved = "{name} enregistré"
outline.empty.hint = "Une intitulé de scène commence par INT. ou EXT."
```

Copy [`assets/lang/en.toml`](assets/lang/en.toml) — the reference file, with
every key in it and comments explaining the format — translate the right-hand
side of each line, and drop it into the languages folder, which the language
window will open for you:

| Platform | Folder |
| --- | --- |
| macOS | `~/Library/Application Support/openwrite/languages` |
| Windows | `%APPDATA%\openwrite\languages` |
| Linux | `~/.config/openwrite/languages` |

It appears in the picker immediately. Nothing is rebuilt and nothing is
installed.

Four things make this usable by a translator rather than only by a programmer:

- **You do not have to finish.** English is compiled into the binary and any key
  a translation has not reached falls back to it, so a file is useful from its
  first line. **Reload the language files** re-reads the folder while the editor
  is running, so the loop is: change a line, press the button, read the window.
- **One bad line costs one line.** A file that will not parse is not refused; the
  lines that do parse are used, and the language window lists what was wrong with
  the rest, by line number.
- **Placeholders are named, never positional.** `{name}`, `{n}`, `{query}` —
  a translation may put them wherever its grammar wants them. `{C}` becomes `⌘`
  or `Ctrl+` on its own.
- **Counting is the language's business.** A counted message gives `.one` and
  `.other` — or `.few` and `.many`, for the Slavic rules — and the file says
  which rule it follows.

Fountain markup is the one thing that does not translate: `INT.`, `EXT.`, the
colon in `MAYA: Forty-one.` and `/transition:` are what the parser reads, and
they stay as they are in every language. The lines where they appear are marked
in the reference file.

Tests hold the files honest: every key the window asks for is in the English
file, every line in the English file is asked for somewhere, and any language
shipped in the binary matches English key for key and placeholder for
placeholder.

### Updates

The editor asks GitHub once, in the background, whether there is a newer
release. If there is, a small window offers the download; if there is not, or
the question could not be asked, nothing is said and nothing is in the way. It
is asked after the window is already up, so a slow network delays no screenplay,
and dismissing it dismisses it.

This is the one thing the program does over the network without being asked, so:
it fetches one small JSON document from `api.github.com`, sends nothing about
you beyond a `User-Agent` of `openwrite`, records the check in the debug log, and
stops entirely if `OPENWRITE_NO_UPDATE_CHECK` is set. Builds made without the
`update` feature never ask at all.

### The debug log

<kbd>⇧⌘L</kbd> shows **what the program has been doing** — files opened and their
sizes, how long each repagination took, what a model server said and how long it
took to say it, and any error that went past. It is a ring buffer in memory,
always running, and it can be copied or saved from the window in one press.

The rule it is written to is worth stating plainly:

> The log records what the program did, never what the writer wrote.

Screenplay text, character names, world notes, the prompts sent to a model and
the answers that come back are all your work, and none of them go in — a log you
might send to somebody to look at should not be carrying your unpublished
script. What goes in is counts, sizes, durations, formats, addresses and error
text: *"2,048 characters of prompt, 61 characters of answer, 1,204 ms"* says
everything needed to debug a model that will not answer and gives away nothing.
There is a test that asks a stub model server a question with a distinctive
phrase in it and asserts the phrase is nowhere in the log.

Repagination happens on every keystroke, so those entries are hidden until you
ask for them — which is exactly when a document has become slow.

Setting `OPENWRITE_LOG` to a path writes every entry to that file as it happens,
which is the only way to see the last entries from a run that ended badly; a
panic is written there too.

### Keyboard shortcuts

`⌘` is `Ctrl` on Windows and Linux.

| | |
|---|---|
| <kbd>⌘N</kbd> / <kbd>⌘O</kbd> / <kbd>⌘S</kbd> | New, open, save |
| <kbd>⇧⌘S</kbd> | Save as |
| <kbd>⌘E</kbd> | Export formatted script (text, HTML or Final Draft) |
| <kbd>⇧⌘C</kbd> | Copy the formatted script to the clipboard |
| <kbd>⌘F</kbd> / <kbd>⌘G</kbd> / <kbd>⇧⌘G</kbd> | Find, find next, find previous |
| <kbd>⌘K</kbd> | Characters and world |
| <kbd>⌘I</kbd> | Ideas from a local model |
| <kbd>⇧⌘A</kbd> | Audio drama |
| <kbd>⌘1</kbd> / <kbd>⌘2</kbd> / <kbd>⌘3</kbd> | Focus the outline, the editor, the preview |
| <kbd>F6</kbd> / <kbd>⇧F6</kbd> | Next / previous pane |
| <kbd>⌘]</kbd> / <kbd>⌘[</kbd> | Next / previous scene |
| <kbd>⇧⌘O</kbd> / <kbd>⇧⌘P</kbd> | Show or hide the outline / the preview |
| <kbd>⇧⌘H</kbd> | High contrast on or off |
| — | Language (**View → Language…**) |
| — | Open, save and record an audio drama (**Audio Drama** menu) |
| <kbd>⌘+</kbd> / <kbd>⌘-</kbd> / <kbd>⌘0</kbd> | Text size |
| <kbd>F1</kbd> | Shortcuts and markup |
| <kbd>⇧⌘L</kbd> | Debug log |

The HTML export is a printable, screen-reader-friendly document in its own
right: scene headings are real headings, each scene is a labelled landmark,
there is a skip link and a scene navigation list, `j`/`k`/`n`/`p` move between
scenes, and the print stylesheet sets US Letter with correct margins. Print it
to PDF for a submission-ready script.

## Output formats

<kbd>⌘E</kbd> exports; the format follows the extension you choose.

| | |
|---|---|
| `.txt` | Fixed-width, exactly the page geometry. |
| `.html` | Printable and accessible; print to PDF for submission. |
| `.fdx` | Final Draft XML, for a production workflow. |

## As a library

`openwrite::parse` reads Fountain and the one-line dialogue form together;
`openwrite::shorthand::expand` turns the latter back into the former, which is
what you want before handing a source string to anything else.

```rust
let doc = openwrite::parse(&source);
let opts = openwrite::layout::Options::default();
let pages = openwrite::layout::paginate(&doc, &opts);
print!("{}", openwrite::render::text::render(&pages, &opts, false));
```

`layout::paginate` returns the pages every renderer works from, so text, HTML
and the on-screen preview can never disagree about where a page breaks.

The formatting engine, the parser, the `.sct` document, the story bible and the
shorthand are all available without the window: `--no-default-features` builds
them with no dependencies at all.

## Downloads

Tagged releases carry a macOS build for Apple Silicon, a Windows build for
x86-64, and a `.deb` for x86-64 Debian- and Ubuntu-based distros, on the
[releases page](../../releases). On macOS it is a `.app` bundle that opens
`.sct` and `.fountain` files from Finder, and audio drama `.xml` files through
**Open With** — it does not make itself the handler for every XML file on the
machine; on Windows it is a single executable; the `.deb` installs the binary to `/usr/bin` with
`sudo apt install ./screenplay-creation-tool-linux-x86_64.deb`.

Neither the macOS nor the Windows download is notarised or signed with a paid
certificate, so the first open needs a nudge: on macOS right-click the app and
choose **Open**; on Windows choose **More info** then **Run anyway** at the
SmartScreen prompt.

## Building

```sh
cargo build --release
cargo test
cargo build --release --no-default-features              # the engine alone, no window
cargo build --release --no-default-features --features gui   # no network at all
```

The formatting engine has no dependencies at all — `--no-default-features`
builds it, and `--self-check`, with nothing else linked in. The application adds
`eframe` for the window and `rfd` for the native file dialogs.

Talking to a local model has no dependencies either: it is a few hundred lines
of HTTP/1.1 and JSON over `std::net`, in [`src/ai/`](src/ai/), rather than a
client library. It is still its own Cargo feature (`ai`, on by default), because
it is the one part of the program that opens a socket and a build that should
not be able to is worth being able to make. The update check is a second feature
(`update`) for the same reason; it shells out to `curl` rather than linking a TLS
stack for one request a session.

The audio drama is a third (`drama`), and has no dependencies either, which is
less obvious than it sounds. Its HTTPS is `curl`, as the update check's is; the
story format is read by a small XML reader in
[`src/drama/story.rs`](src/drama/story.rs); and the pitch shifting, the tremble,
the panning and the mixing in
[`src/drama/audio.rs`](src/drama/audio.rs) are arithmetic on 16-bit samples,
which is why it asks ElevenLabs for PCM rather than the MP3 it sends by default.
It is the one part of the program that costs money, so it is also the one most
worth being able to compile out.

Releases are built by [`.github/workflows/release.yml`](.github/workflows/release.yml),
which runs the tests, has the binary it just built check itself over a sample
screenplay (`--self-check`, since a window cannot be opened on a build runner),
and verifies the executable links nothing that will not exist on a user's
machine. Pushing a `v*` tag publishes; running the workflow by hand
builds and uploads the artifacts without publishing anything.

## Fountain support

Everything below is standard [Fountain](https://fountain.io), with the two
departures noted at the end.

Title pages, scene headings (including forced `.headings` and `#1A#` scene
numbers), action, character cues with extensions, dialogue, parentheticals,
lyrics, dual dialogue, transitions, centred text, page breaks, sections and
synopses, notes, the boneyard, and `*italic*` `**bold**` `_underline_` emphasis.

Sections and synopses are parsed and shown in the outline but never printed —
they are notes to yourself, not part of the script.

### The two departures

**A speech can be written on one line**, along with `/transition:`, as described
[above](#typing-dialogue-quickly). This is the tool's own and is expanded back
into the three-line Fountain form whenever a `.fountain` file is written, so a
file leaving here is one any other reader agrees with. The `.sct` document keeps
what you typed.

**A title page needs a title-page-shaped first key.** Fountain treats any
`Key: value` first line as a title page; here that key has to be either an
ordinary capitalised word (`Title`, `Credit`, `Production`) or one of the usual
keys spelled in capitals (`TITLE:`). That is what lets `MAYA: Forty-one.` typed
into an empty document be Maya speaking rather than a title page with a key
called MAYA. Every real title page is unaffected.

## Licence

GNU General Public License, version 3 or later. The full text is in
[`LICENSE`](LICENSE).

This program is free software: you can redistribute it and modify it under
those terms. It comes with no warranty.

The bundled Ubuntu Bold typeface is a separate work under the Ubuntu Font
Licence 1.0, in [`assets/fonts/`](assets/fonts/).
