import re
import tomllib
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
SRC = REPO_ROOT / "src"
TUNE_RS = SRC / "tune.rs"
TUNE_TOML = REPO_ROOT / "tools" / "auto_tune" / "tune.toml"
AUTO_TUNING_DOC = REPO_ROOT / "docs" / "auto-tuning.md"

# A constant read only behind a non-default feature is not tunable by the
# default-feature build, so it must stay out of tune.toml.
DEFAULT_FEATURES = {"mimalloc"}

NAME_ARM_RE = re.compile(r"TuneParam::(\w+)\s*=>\s*\"([A-Z][A-Z_0-9]*)\"")
CONST_RE = re.compile(
    r"(?:pub(?:\([^)]*\))?\s+)?const\s+(\w+)\s*:\s*[iu](?:8|16|32|64|size)\s*=\s*(-?[\d_]+)\s*;"
)
DOC_ROW_RE = re.compile(r"^\|\s*`([A-Z][A-Z_0-9]*)`\s*\|\s*(-?[\d_]+)\s*\|", re.MULTILINE)


def _matching_brace(text, open_index):
    depth = 0
    for index in range(open_index, len(text)):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return index
    raise ValueError("unbalanced braces in source text")


def _split_top_level(args):
    depth = 0
    for index, char in enumerate(args):
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == "," and depth == 0:
            return args[:index].strip(), args[index + 1 :].strip()
    raise ValueError(f"call has no argument separator: {args!r}")


def _strip_trailing_comma(text):
    return text.strip().rstrip(",").strip()


def _iter_get_int_calls(text):
    for match in re.finditer(r"tune::get_int\s*\(", text):
        start = match.end()
        depth = 1
        index = start
        while index < len(text) and depth:
            char = text[index]
            if char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
            index += 1
        if depth:
            raise ValueError("unbalanced tune::get_int call")
        yield match.start(), text[start : index - 1]


def _enclosing_gates(text, position):
    features = set()
    in_test = False
    for match in re.finditer(r"#\[cfg\(([^)]*)\)\]", text):
        if match.start() >= position:
            continue
        body = match.group(1)
        scope_start = _cfg_scope_start(text, match.end())
        if scope_start is None or scope_start >= position:
            continue
        if not _block_still_open(text, scope_start, position):
            continue
        if "test" in body:
            in_test = True
            continue
        feature = re.search(r'feature\s*=\s*"([^"]+)"', body)
        if feature is None:
            continue
        if re.search(r"not\s*\(", body):
            continue
        features.add(feature.group(1))
    return features, in_test


def _cfg_scope_start(text, attribute_end):
    index = attribute_end
    length = len(text)
    while index < length:
        char = text[index]
        if char in " \t\r\n":
            index += 1
            continue
        if char == "{":
            return index
        if char == ";":
            return None
        while index < length and text[index] not in "{;":
            if text[index] == "}" or text[index] == "(":
                return None
            index += 1
        if index < length and text[index] == "{":
            return index
        return None
    return None


def _block_still_open(text, open_index, position):
    depth = 0
    index = open_index
    length = len(text)
    while index < position:
        char = text[index]
        if char == "/" and index + 1 < length and text[index + 1] == "/":
            newline = text.find("\n", index)
            index = length if newline == -1 else newline
            continue
        if char == "/" and index + 1 < length and text[index + 1] == "*":
            end = text.find("*/", index + 2)
            index = length if end == -1 else end + 2
            continue
        if char == '"':
            index = _skip_string(text, index, '"')
            continue
        if char == "'":
            index = _skip_string(text, index, "'")
            continue
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return False
        index += 1
    return depth > 0


def _skip_string(text, index, quote):
    length = len(text)
    index += 1
    while index < length:
        char = text[index]
        if char == "\\":
            index += 2
            continue
        if char == quote:
            return index + 1
        index += 1
    return length


def _collect_constants():
    values = {}
    for path in sorted(SRC.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for match in CONST_RE.finditer(text):
            name = match.group(1)
            value = int(match.group(2).replace("_", ""))
            if name in values and values[name] != value:
                raise AssertionError(
                    f"constant {name} has conflicting values in the Rust sources"
                )
            values[name] = value
    return values


def _resolve_default(expression, constants):
    expression = _strip_trailing_comma(expression)
    literal = re.fullmatch(r"-?[\d_]+", expression)
    if literal:
        return int(expression.replace("_", ""))
    from_const = re.fullmatch(r"i64::from\((\w+)\)", expression)
    if from_const:
        return constants[from_const.group(1)]
    cast = re.fullmatch(r"(\w+)\s+as\s+i64", expression)
    if cast:
        return constants[cast.group(1)]
    identifier = re.fullmatch(r"\w+", expression)
    if identifier:
        return constants[expression]
    raise AssertionError(
        f"unsupported default expression {expression!r}; extend the parser"
    )


def _collect_code_defaults():
    constants = _collect_constants()
    defaults = {}
    for path in sorted(SRC.rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for position, args in _iter_get_int_calls(text):
            param_arg, default_arg = _split_top_level(args)
            param_arg = _strip_trailing_comma(param_arg)
            match = re.fullmatch(r"TuneParam::(\w+)", param_arg)
            if not match:
                raise AssertionError(f"unexpected tune parameter argument: {param_arg!r}")
            param = match.group(1)
            value = _resolve_default(default_arg, constants)
            features, in_test = _enclosing_gates(text, position)
            if in_test:
                continue
            gates = features - DEFAULT_FEATURES
            if param in defaults and defaults[param] != (value, gates):
                raise AssertionError(
                    f"parameter {param} has conflicting defaults in the Rust sources"
                )
            defaults[param] = (value, gates)
    return defaults


def _collect_enum_variants():
    text = TUNE_RS.read_text(encoding="utf-8")
    enum_start = text.index("pub enum TuneParam {")
    enum_end = _matching_brace(text, enum_start)
    variants = []
    for line in text[enum_start + len("pub enum TuneParam {") : enum_end].splitlines():
        match = re.fullmatch(r"\s*([A-Z][A-Za-z0-9]*),\s*", line)
        if match:
            variants.append(match.group(1))

    name_start = text.index("pub fn name(self)")
    name_end = _matching_brace(text, text.index("{", name_start))
    names = dict(
        (variant, name)
        for variant, name in NAME_ARM_RE.findall(text[name_start:name_end])
    )
    return variants, names


def _collect_from_name_names():
    text = TUNE_RS.read_text(encoding="utf-8")
    start = text.index("pub fn from_name(name: &str)")
    end = _matching_brace(text, text.index("{", start))
    body = text[start:end]
    accepted = set()
    for arm in re.finditer(r'"([A-Z][A-Z_0-9]*)"', body):
        accepted.add(arm.group(1))
    return accepted


def _collect_doc_defaults():
    text = AUTO_TUNING_DOC.read_text(encoding="utf-8")
    defaults = {}
    for name, raw in DOC_ROW_RE.findall(text):
        value = int(raw.replace("_", ""))
        if name in defaults and defaults[name] != value:
            raise AssertionError(
                f"parameter {name} is documented twice with different defaults"
            )
        defaults[name] = value
    return defaults


def _collect_tuner_params():
    config = tomllib.loads(TUNE_TOML.read_text(encoding="utf-8"))
    return {spec["name"]: spec for spec in config["params"]}


class TuneParameterConsistencyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.variants, cls.names = _collect_enum_variants()
        cls.code_defaults = _collect_code_defaults()
        cls.doc_defaults = _collect_doc_defaults()
        cls.tuner_params = _collect_tuner_params()

    def test_enum_variants_all_have_a_unique_uci_name(self):
        self.assertTrue(self.variants, "no TuneParam variants were parsed")
        self.assertEqual(len(self.variants), len(set(self.variants)))
        self.assertEqual(
            set(self.variants),
            set(self.names),
            "every TuneParam variant needs an arm in TuneParam::name",
        )
        self.assertEqual(
            len(self.names),
            len(set(self.names.values())),
            "two TuneParam variants share a UCI name",
        )

    def test_every_parameter_name_is_accepted_by_from_name(self):
        accepted = _collect_from_name_names()
        for variant in self.variants:
            with self.subTest(parameter=variant):
                name = self.names[variant]
                self.assertIn(
                    name,
                    accepted,
                    f"{name} is missing from TuneParam::from_name",
                )

    def test_every_parameter_is_read_from_the_tuning_table(self):
        self.assertEqual(
            set(self.code_defaults),
            set(self.names),
            "every TuneParam variant needs a tune::get_int read site",
        )

    def test_documented_defaults_match_the_code(self):
        for variant in self.variants:
            name = self.names[variant]
            with self.subTest(parameter=name):
                self.assertIn(
                    name,
                    self.doc_defaults,
                    f"{name} is missing from the documented parameter tables",
                )
                self.assertEqual(
                    self.doc_defaults[name],
                    self.code_defaults[variant][0],
                    f"{name}: docs/auto-tuning.md disagrees with the code default",
                )

    def test_tuner_base_values_match_the_code(self):
        for variant in self.variants:
            name = self.names[variant]
            if name not in self.tuner_params:
                continue
            with self.subTest(parameter=name):
                self.assertEqual(
                    self.tuner_params[name]["base"],
                    self.code_defaults[variant][0],
                    f"{name}: tune.toml base disagrees with the code default",
                )

    def test_tuner_covers_exactly_the_default_build_parameters(self):
        tunable = {
            self.names[variant]
            for variant, (_, gates) in self.code_defaults.items()
            if not gates
        }
        feature_gated = {
            self.names[variant]
            for variant, (_, gates) in self.code_defaults.items()
            if gates
        }
        self.assertEqual(
            set(self.tuner_params),
            tunable,
            "tune.toml must list every parameter the default build can tune",
        )
        self.assertFalse(
            feature_gated & set(self.tuner_params),
            "tune.toml tunes default-feature builds only; feature-gated "
            "parameters must stay out of it",
        )

    def test_tuner_ranges_contain_the_base_value(self):
        for name, spec in self.tuner_params.items():
            with self.subTest(parameter=name):
                self.assertLessEqual(spec["min"], spec["base"])
                self.assertLessEqual(spec["base"], spec["max"])


if __name__ == "__main__":
    unittest.main()
