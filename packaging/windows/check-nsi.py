#!/usr/bin/env python3
"""Static checks of rustypresenter.nsi, for machines without makensis (the cloud, Linux CI).

    python3 packaging/windows/check-nsi.py [file.nsi]

Not a compiler. It catches what a typo or a careless edit breaks: unbalanced blocks, an unknown instruction,
a Call or page callback to a function that does not exist, an installer function called from the uninstaller
(NSIS requires "un." there), a jump to a missing label, an undefined ${NAME} or $Var, an unknown MessageBox flag,
pages after MUI_LANGUAGE, and the uninstaller removing a folder with /r other than the data folder.
makensis on the PC is the real check; make-installer.cmd runs it.
"""
import re
import sys
from pathlib import Path

NSI = Path(sys.argv[1] if len(sys.argv) > 1 else Path(__file__).with_name("rustypresenter.nsi"))

# Instructions and attributes this script may use; anything else is reported as unknown.
INSTRUCTIONS = {
    "Unicode", "ManifestDPIAware", "SetCompressor", "RequestExecutionLevel", "Name", "OutFile", "InstallDir",
    "InstallDirRegKey", "BrandingText", "ShowInstDetails", "ShowUninstDetails", "VIProductVersion",
    "VIAddVersionKey", "Var", "UninstPage", "Page", "Function", "FunctionEnd", "Section", "SectionEnd", "SectionIn",
    "Call", "SetOutPath", "File", "WriteUninstaller", "CreateShortCut", "WriteRegStr", "WriteRegDWORD",
    "DeleteRegKey", "Delete", "RMDir", "IntFmt", "Pop", "Push", "Abort", "ClearErrors", "MessageBox",
    "SetShellVarContext", "DetailPrint", "Exec", "Goto", "Quit",
}
# Plugins that come with NSIS.
PLUGIN_CALLS = {"nsDialogs::Create", "nsDialogs::Show"}
# Macros from the headers this script includes (LogicLib, MUI2, nsDialogs, FileFunc) and the -D values
# make-installer.sh passes.
HEADER_DEFINES = {
    "If", "ElseIf", "Else", "EndIf", "FileExists", "Errors", "GetSize", "NSD_CreateLabel", "NSD_CreateCheckbox",
    "NSD_SetState", "NSD_GetState", "BST_CHECKED", "BST_UNCHECKED",
}
PASSED = {"VERSION", "VIVERSION", "FILES_NSH", "UNFILES_NSH", "OUTFILE", "ICON"}
BUILTIN_VARS = {"INSTDIR", "LOCALAPPDATA", "SMPROGRAMS", "DESKTOP", "TEMP", "PROGRAMFILES", "APPDATA", "EXEDIR",
                "PLUGINSDIR", "OUTDIR", "\\r", "\\n", "\\t", "\\\""} | {str(i) for i in range(10)} | {f"R{i}" for i in range(10)}
MB_FLAGS = {"MB_OK", "MB_OKCANCEL", "MB_RETRYCANCEL", "MB_YESNO", "MB_YESNOCANCEL", "MB_ABORTRETRYIGNORE",
            "MB_ICONEXCLAMATION", "MB_ICONINFORMATION", "MB_ICONQUESTION", "MB_ICONSTOP", "MB_TOPMOST",
            "MB_SETFOREGROUND", "MB_DEFBUTTON1", "MB_DEFBUTTON2", "MB_DEFBUTTON3"}
MB_RETURNS = {"IDOK", "IDCANCEL", "IDRETRY", "IDYES", "IDNO", "IDABORT", "IDIGNORE"}

errors = []


def err(n, msg):
    errors.append(f"{NSI.name}:{n}: {msg}")


def logical_lines(text):
    """(line number, text) with comments removed and `\\` continuations joined."""
    out, buf, start = [], "", 0
    for n, raw in enumerate(text.splitlines(), 1):
        line = raw.rstrip()
        stripped = line.lstrip()
        if stripped.startswith(";") or stripped.startswith("#"):
            line = ""
        else:
            line = strip_comment(line)
        if not buf:
            start = n
        if line.endswith("\\"):
            buf += line[:-1] + " "
            continue
        buf += line
        if buf.strip():
            out.append((start, buf.strip()))
        buf = ""
    return out


def strip_comment(line):
    """Drops a `;` comment that is outside quotes."""
    quote = None
    for i, c in enumerate(line):
        if quote:
            if c == quote:
                quote = None
        elif c in "\"'`":
            quote = c
        elif c == ";":
            return line[:i].rstrip()
    return line


def tokens(line):
    return re.findall(r'"[^"]*"|\'[^\']*\'|`[^`]*`|\S+', line)


def expand_macros(lines):
    """Replaces each !insertmacro of a macro defined in this file with its body, parameters filled in."""
    macros, out, i = {}, [], 0
    while i < len(lines):
        n, line = lines[i]
        t = tokens(line)
        if t[0] == "!macro":
            name, params, body = t[1], t[2:], []
            i += 1
            while i < len(lines) and tokens(lines[i][1])[0] != "!macroend":
                body.append(lines[i])
                i += 1
            if i == len(lines):
                err(n, f"!macro {name} has no !macroend")
            macros[name] = (params, body)
            i += 1
            continue
        if t[0] == "!insertmacro" and t[1] in macros:
            params, body = macros[t[1]]
            args = [a.strip('"') for a in t[2:]]
            if len(args) != len(params):
                err(n, f"!insertmacro {t[1]} needs {len(params)} argument(s)")
            for bn, bline in body:
                for p, a in zip(params, args):
                    bline = bline.replace("${%s}" % p, a)
                out.append((bn, bline))
            i += 1
            continue
        out.append((n, line))
        i += 1
    return out


def main():
    text = NSI.read_text(encoding="utf-8")
    lines = expand_macros(logical_lines(text))
    defines = set(HEADER_DEFINES) | PASSED
    variables = set(BUILTIN_VARS)
    functions = {}          # name -> line
    calls = []              # (line, caller context, callee)
    callbacks = []          # (line, is_uninstaller_page, function)
    labels = {}             # function/section -> set of labels
    jumps = []              # (line, block, label)
    stack = []              # open blocks: (kind, line)
    block = None            # current Function/Section name
    mui_language_at = None
    for n, line in lines:
        t = tokens(line)
        word = t[0]
        # Preprocessor.
        if word.startswith("!"):
            if word in ("!ifdef", "!ifndef", "!if"):
                stack.append(("!if", n))
            elif word in ("!else",):
                if not stack or stack[-1][0] != "!if":
                    err(n, "!else outside !if")
            elif word == "!endif":
                if not stack or stack[-1][0] != "!if":
                    err(n, "!endif without !if")
                else:
                    stack.pop()
            elif word == "!define":
                defines.add(t[1])
            elif word == "!insertmacro":
                if t[1] == "MUI_LANGUAGE":
                    mui_language_at = n
                elif t[1].startswith(("MUI_PAGE_", "MUI_UNPAGE_")) and mui_language_at:
                    err(n, f"{t[1]} after MUI_LANGUAGE (line {mui_language_at})")
            elif word not in ("!include", "!error", "!warning", "!echo", "!insertmacro"):
                err(n, f"unknown directive {word}")
            check_refs(n, line, defines, variables, t)
            continue
        # Labels.
        if re.fullmatch(r"[A-Za-z_][\w.]*:", line):
            labels.setdefault(block, set()).add(line[:-1])
            continue
        # LogicLib.
        if word.startswith("${"):
            name = word[2:-1]
            if name == "If":
                stack.append(("If", n))
            elif name == "EndIf":
                if not stack or stack[-1][0] != "If":
                    err(n, "${EndIf} without ${If}")
                else:
                    stack.pop()
            elif name in ("Else", "ElseIf"):
                if not stack or stack[-1][0] != "If":
                    err(n, f"${{{name}}} outside ${{If}}")
            check_refs(n, line, defines, variables, t)
            continue
        if "::" in word:
            if word not in PLUGIN_CALLS:
                err(n, f"unknown plugin call {word} (only plugins that come with NSIS)")
            continue
        if word not in INSTRUCTIONS:
            err(n, f"unknown instruction {word}")
            continue
        if word == "Function":
            if block:
                err(n, f"Function inside {block}")
            block = t[1]
            functions[t[1]] = n
            stack.append(("Function", n))
        elif word == "Section":
            if block:
                err(n, f"Section inside {block}")
            name = t[1].strip('"')
            block = "Uninstall" if name in ("Uninstall", "un.") or name.startswith("un.") else f"Section {name}"
            stack.append(("Section", n))
        elif word in ("FunctionEnd", "SectionEnd"):
            kind = word[:-3]
            if not stack or stack[-1][0] != kind:
                err(n, f"{word} does not close a {kind} (open: {stack[-1] if stack else 'nothing'})")
            else:
                stack.pop()
            block = None
        elif word == "Var":
            variables.add(t[-1])
        elif word == "Call":
            calls.append((n, block, t[1]))
        elif word in ("UninstPage", "Page") and t[1] == "custom":
            for f in t[2:4]:
                callbacks.append((n, word == "UninstPage", f))
        elif word == "MessageBox":
            for flag in t[1].split("|"):
                if flag not in MB_FLAGS:
                    err(n, f"unknown MessageBox flag {flag}")
            rest = t[3:]
            if rest[:1] == ["/SD"]:
                if rest[1] not in MB_RETURNS:
                    err(n, f"unknown /SD value {rest[1]}")
                rest = rest[2:]
            for ret, label in zip(rest[::2], rest[1::2]):
                if ret not in MB_RETURNS:
                    err(n, f"unknown MessageBox return {ret}")
                jumps.append((n, block, label))
        elif word == "Goto":
            jumps.append((n, block, t[1]))
        elif word == "RMDir" and "/r" in t and block == "Uninstall" and "${DATADIR}" not in line:
            err(n, "the uninstaller removes a folder with /r; only the data folder, behind its checkbox, may be")
        if word in ("Function", "Section") and block is None:
            pass
        check_refs(n, line, defines, variables, t)

    for kind, n in stack:
        err(n, f"{kind} is never closed")
    for n, caller, callee in calls:
        if callee not in functions:
            err(n, f"Call {callee}: no such function")
        in_uninstaller = caller == "Uninstall" or (caller or "").startswith("un.")
        if in_uninstaller != callee.startswith("un."):
            err(n, f"Call {callee} from {caller}: the uninstaller may only call un. functions, the installer none")
    for n, uninstaller, f in callbacks:
        if f not in functions:
            err(n, f"page callback {f}: no such function")
        elif uninstaller != f.startswith("un."):
            err(n, f"page callback {f}: uninstaller pages need un. functions")
    for n, where, label in jumps:
        if label.startswith(("+", "-")) or label == "0":
            continue
        if label not in labels.get(where, set()):
            err(n, f"jump to {label}: no such label in {where}")
    if mui_language_at is None:
        err(0, "no MUI_LANGUAGE")
    for name in (".onInit", "un.onInit"):
        if name not in functions:
            err(0, f"no {name}: shortcuts would follow the default shell context")

    if errors:
        print("\n".join(errors))
        sys.exit(1)
    print(f"{NSI.name}: ok ({len(lines)} lines, {len(functions)} functions)")


def check_refs(n, line, defines, variables, t):
    # Not the name being defined on a !define line.
    body = line
    if t[0] == "!define" and len(t) > 2:
        body = " ".join(t[2:])
    elif t[0] == "!define":
        return
    for name in re.findall(r"\$\{([^}]+)\}", body):
        if name not in defines:
            err(n, f"${{{name}}} is not defined")
    for name in re.findall(r"\$(\\[rnt\"]|[A-Za-z_][\w]*|\d)", re.sub(r"\$\{[^}]+\}", "", body)):
        if name not in variables:
            err(n, f"${name} is not a declared variable")


if __name__ == "__main__":
    main()
