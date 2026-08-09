"""Static Python <-> Python Flow conversion for RPAZ entrypoints.

The converter is deliberately syntax-only: it parses source with :mod:`ast`,
never imports the source module, and never evaluates user expressions.  Schema
v1 keeps both structured control data for a visual editor and the original
statement text for lossless fallback through ``raw-code`` nodes.
"""

from __future__ import annotations

import ast
import hashlib
import json
import re
import sys
from dataclasses import dataclass, field
from typing import Any, Mapping, Sequence


SCHEMA_VERSION = 1
FLOW_KIND = "drpa.python-flow"
MAX_SOURCE_BYTES = 2 * 1024 * 1024
MAX_JSON_BYTES = 4 * 1024 * 1024
NODE_TYPES = frozenset(
    {
        "start",
        "end",
        "assign",
        "call",
        "ctx-call",
        "rpa-call",
        "if",
        "for",
        "while",
        "try",
        "return",
        "raw-code",
    }
)
_SAFE_SOURCE_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}\.py$")


class PythonFlowError(ValueError):
    """Base exception for conversion and schema errors."""


class PythonFlowSyntaxError(PythonFlowError):
    """A syntax error enriched with a stable source location."""

    def __init__(
        self,
        message: str,
        *,
        source_name: str,
        line: int | None,
        column: int | None,
    ) -> None:
        self.source_name = source_name
        self.line = line
        self.column = column
        location = source_name
        if line is not None:
            location += f":{line}"
            if column is not None:
                location += f":{column}"
        super().__init__(f"{location}: {message}")


class PythonFlowValidationError(PythonFlowError):
    """A Python Flow document does not satisfy schema v1."""


@dataclass(slots=True)
class SourceSpan:
    """One-based lines and zero-based columns, matching Python AST locations."""

    start_line: int
    start_column: int
    end_line: int
    end_column: int

    def to_dict(self) -> dict[str, int]:
        return {
            "startLine": self.start_line,
            "startColumn": self.start_column,
            "endLine": self.end_line,
            "endColumn": self.end_column,
        }

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "SourceSpan":
        try:
            return cls(
                start_line=int(value["startLine"]),
                start_column=int(value["startColumn"]),
                end_line=int(value["endLine"]),
                end_column=int(value["endColumn"]),
            )
        except (KeyError, TypeError, ValueError) as exc:
            raise PythonFlowValidationError("node span is not a valid source span") from exc


@dataclass(slots=True)
class FlowNode:
    id: str
    type: str
    label: str
    code: str
    span: SourceSpan
    data: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "type": self.type,
            "label": self.label,
            "code": self.code,
            "span": self.span.to_dict(),
            "data": self.data,
        }

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "FlowNode":
        try:
            data = value.get("data", {})
            if not isinstance(data, Mapping):
                raise TypeError("data")
            return cls(
                id=str(value["id"]),
                type=str(value["type"]),
                label=str(value.get("label", "")),
                code=str(value.get("code", "")),
                span=SourceSpan.from_dict(_mapping(value["span"], "node span")),
                data=dict(data),
            )
        except (KeyError, TypeError, ValueError) as exc:
            if isinstance(exc, PythonFlowValidationError):
                raise
            raise PythonFlowValidationError("node is not a valid schema v1 node") from exc


@dataclass(slots=True)
class FlowEdge:
    id: str
    source: str
    target: str
    kind: str = "next"
    label: str = ""

    def to_dict(self) -> dict[str, str]:
        return {
            "id": self.id,
            "source": self.source,
            "target": self.target,
            "kind": self.kind,
            "label": self.label,
        }

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "FlowEdge":
        try:
            return cls(
                id=str(value["id"]),
                source=str(value["source"]),
                target=str(value["target"]),
                kind=str(value.get("kind", "next")),
                label=str(value.get("label", "")),
            )
        except (KeyError, TypeError, ValueError) as exc:
            raise PythonFlowValidationError("edge is not a valid schema v1 edge") from exc


@dataclass(slots=True)
class PythonFlow:
    schema_version: int
    kind: str
    source_name: str
    source_hash: str
    entrypoint: str
    nodes: list[FlowNode]
    edges: list[FlowEdge]
    metadata: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return {
            "schemaVersion": self.schema_version,
            "kind": self.kind,
            "source": {"name": self.source_name, "sha256": self.source_hash},
            "entrypoint": self.entrypoint,
            "nodes": [node.to_dict() for node in self.nodes],
            "edges": [edge.to_dict() for edge in self.edges],
            "metadata": self.metadata,
        }

    @classmethod
    def from_dict(cls, value: Mapping[str, Any]) -> "PythonFlow":
        try:
            source = _mapping(value["source"], "source")
            nodes = value["nodes"]
            edges = value["edges"]
            metadata = value.get("metadata", {})
            if not isinstance(nodes, list) or not isinstance(edges, list):
                raise TypeError("nodes and edges must be arrays")
            if not isinstance(metadata, Mapping):
                raise TypeError("metadata must be an object")
            return cls(
                schema_version=int(value["schemaVersion"]),
                kind=str(value["kind"]),
                source_name=str(source["name"]),
                source_hash=str(source.get("sha256", "")),
                entrypoint=str(value["entrypoint"]),
                nodes=[FlowNode.from_dict(_mapping(item, "node")) for item in nodes],
                edges=[FlowEdge.from_dict(_mapping(item, "edge")) for item in edges],
                metadata=dict(metadata),
            )
        except (KeyError, TypeError, ValueError) as exc:
            if isinstance(exc, PythonFlowValidationError):
                raise
            raise PythonFlowValidationError("document is not a Python Flow schema v1 object") from exc


def python_to_flow(
    source: str,
    *,
    source_name: str = "main.py",
    entrypoint: str = "main",
) -> PythonFlow:
    """Convert inline RPAZ Python source to a validated schema v1 flow.

    ``source_name`` is metadata only and must be a plain ``.py`` basename.  No
    filesystem path is accepted and no source code is imported or executed.
    """

    _validate_source_name(source_name)
    if not isinstance(source, str):
        raise TypeError("source must be inline Python text")
    if len(source.encode("utf-8")) > MAX_SOURCE_BYTES:
        raise PythonFlowError(f"source exceeds {MAX_SOURCE_BYTES} UTF-8 bytes")
    if not isinstance(entrypoint, str) or not entrypoint.isidentifier():
        raise PythonFlowError("entrypoint must be a Python identifier")
    try:
        tree = ast.parse(source, filename=source_name, mode="exec", type_comments=True)
    except SyntaxError as exc:
        raise PythonFlowSyntaxError(
            exc.msg,
            source_name=source_name,
            line=exc.lineno,
            column=exc.offset,
        ) from exc

    function = next(
        (
            node
            for node in tree.body
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
            and node.name == entrypoint
        ),
        None,
    )
    if function is None:
        raise PythonFlowValidationError(
            f"RPAZ source must define entrypoint {entrypoint}(ctx)"
        )

    builder = _FlowBuilder(source, source_name, entrypoint, tree, function)
    flow = builder.build()
    return validate_flow(flow)


def validate_flow(flow: PythonFlow | Mapping[str, Any]) -> PythonFlow:
    """Validate and return a normalized schema v1 :class:`PythonFlow`."""

    document = flow if isinstance(flow, PythonFlow) else PythonFlow.from_dict(flow)
    if document.schema_version != SCHEMA_VERSION:
        raise PythonFlowValidationError(
            f"unsupported schemaVersion {document.schema_version}; expected {SCHEMA_VERSION}"
        )
    if document.kind != FLOW_KIND:
        raise PythonFlowValidationError(f"unsupported flow kind: {document.kind}")
    _validate_source_name(document.source_name)
    if not document.entrypoint.isidentifier():
        raise PythonFlowValidationError("entrypoint must be a Python identifier")

    node_ids: set[str] = set()
    start_nodes: list[FlowNode] = []
    end_nodes: list[FlowNode] = []
    for node in document.nodes:
        if not node.id or node.id in node_ids:
            raise PythonFlowValidationError(f"duplicate or empty node id: {node.id!r}")
        node_ids.add(node.id)
        if node.type not in NODE_TYPES:
            raise PythonFlowValidationError(f"unsupported node type: {node.type}")
        _validate_span(node.span, node.id)
        if node.type == "start":
            start_nodes.append(node)
        elif node.type == "end":
            end_nodes.append(node)

    if len(start_nodes) != 1 or len(end_nodes) != 1:
        raise PythonFlowValidationError("flow must contain exactly one start and one end node")

    edge_ids: set[str] = set()
    for edge in document.edges:
        if not edge.id or edge.id in edge_ids:
            raise PythonFlowValidationError(f"duplicate or empty edge id: {edge.id!r}")
        edge_ids.add(edge.id)
        if edge.source not in node_ids:
            raise PythonFlowValidationError(
                f"edge {edge.id} references missing source node {edge.source}"
            )
        if edge.target not in node_ids:
            raise PythonFlowValidationError(
                f"edge {edge.id} references missing target node {edge.target}"
            )
        if not edge.kind:
            raise PythonFlowValidationError(f"edge {edge.id} has an empty kind")

    body = document.metadata.get("body")
    if not isinstance(body, list):
        raise PythonFlowValidationError("metadata.body must be an ordered node id array")
    _validate_node_references(body, node_ids, "metadata.body")
    for node in document.nodes:
        _validate_structured_node(node, node_ids)
    return document


def flow_to_python(flow: PythonFlow | Mapping[str, Any]) -> str:
    """Generate valid Python source from a schema v1 flow.

    The ordered child lists in control-node ``data`` are authoritative.  Simple
    and ``raw-code`` nodes use their ``code`` field, allowing visual-editor
    changes to be written back without evaluating any expression.
    """

    document = validate_flow(flow)
    by_id = {node.id: node for node in document.nodes}
    metadata = document.metadata
    function_header = metadata.get("functionHeader")
    if not isinstance(function_header, str) or not function_header.strip():
        function_header = f"def {document.entrypoint}(ctx):"
    module_before = metadata.get("moduleBefore", "")
    module_after = metadata.get("moduleAfter", "")
    if not isinstance(module_before, str) or not isinstance(module_after, str):
        raise PythonFlowValidationError("moduleBefore and moduleAfter must be strings")
    indent = metadata.get("indent", "    ")
    if not isinstance(indent, str) or not indent or indent.strip(" \t"):
        raise PythonFlowValidationError("metadata.indent must contain only spaces or tabs")

    body_ids = metadata["body"]
    body_lines = _render_sequence(body_ids, by_id, level=1, indent=indent)
    if not body_lines:
        body_lines = [f"{indent}pass"]
    function_source = function_header.rstrip() + "\n" + "\n".join(body_lines)
    return module_before + function_source + module_after


def flow_to_json(flow: PythonFlow | Mapping[str, Any], *, indent: int | None = None) -> str:
    """Serialize a validated flow as UTF-8-safe JSON text."""

    return json.dumps(
        validate_flow(flow).to_dict(),
        ensure_ascii=False,
        indent=indent,
        separators=None if indent is not None else (",", ":"),
    )


def flow_from_json(payload: str | bytes) -> PythonFlow:
    """Parse and validate an in-memory JSON flow document."""

    value = _load_json(payload)
    return validate_flow(_mapping(value, "flow"))


def handle_json_request(payload: str | bytes | Mapping[str, Any]) -> str:
    """Handle one CLI-style JSON request and return one JSON response.

    Operations are ``python-to-flow``, ``flow-to-python`` and
    ``validate-flow``.  Only inline ``source``/``flow`` values are accepted;
    path-bearing requests are rejected and the helper performs no file I/O.
    """

    try:
        request = (
            dict(payload)
            if isinstance(payload, Mapping)
            else _mapping(_load_json(payload), "request")
        )
        forbidden = {"path", "sourcePath", "filePath", "inputPath", "outputPath"}
        if forbidden.intersection(request):
            raise PythonFlowError("only inline source or flow JSON is accepted; path input is unsupported")
        operation = request.get("operation")
        if operation == "python-to-flow":
            if "source" not in request:
                raise PythonFlowError("python-to-flow requires inline source")
            result = python_to_flow(
                request["source"],
                source_name=request.get("sourceName", "main.py"),
                entrypoint=request.get("entrypoint", "main"),
            )
            response: dict[str, Any] = {"ok": True, "flow": result.to_dict()}
        elif operation == "flow-to-python":
            response = {"ok": True, "source": flow_to_python(_request_flow(request))}
        elif operation == "validate-flow":
            validate_flow(_request_flow(request))
            response = {"ok": True, "valid": True}
        else:
            raise PythonFlowError(f"unsupported operation: {operation!r}")
    except (PythonFlowError, TypeError, json.JSONDecodeError) as exc:
        response = {
            "ok": False,
            "error": str(exc),
            "errorType": type(exc).__name__,
        }
    return json.dumps(response, ensure_ascii=False, separators=(",", ":"))


def main() -> int:
    """Run one bounded JSON request over stdin/stdout for the desktop Host."""

    payload = sys.stdin.buffer.read(MAX_JSON_BYTES + 1)
    sys.stdout.write(handle_json_request(payload))
    sys.stdout.write("\n")
    return 0


class _FlowBuilder:
    def __init__(
        self,
        source: str,
        source_name: str,
        entrypoint: str,
        tree: ast.Module,
        function: ast.FunctionDef | ast.AsyncFunctionDef,
    ) -> None:
        self.source = source
        self.source_name = source_name
        self.entrypoint = entrypoint
        self.tree = tree
        self.function = function
        self.lines = source.splitlines(keepends=True)
        self.nodes: list[FlowNode] = []
        self.edges: list[FlowEdge] = []
        self.node_by_id: dict[str, FlowNode] = {}
        self.fingerprint_counts: dict[str, int] = {}
        self.edge_keys: set[tuple[str, str, str]] = set()
        self.rpa_aliases, self.rpa_functions = _collect_rpa_aliases(tree)

    def build(self) -> PythonFlow:
        start_span = SourceSpan(
            self.function.lineno,
            self.function.col_offset,
            self.function.lineno,
            self.function.col_offset,
        )
        end_span = SourceSpan(
            self.function.end_lineno or self.function.lineno,
            self.function.end_col_offset or self.function.col_offset,
            self.function.end_lineno or self.function.lineno,
            self.function.end_col_offset or self.function.col_offset,
        )
        seed = f"{self.source_name}:{self.entrypoint}"
        start = FlowNode(
            id=f"start-{_digest(seed)}",
            type="start",
            label="Start",
            code="",
            span=start_span,
            data={"entrypoint": self.entrypoint},
        )
        end = FlowNode(
            id=f"end-{_digest(seed)}",
            type="end",
            label="End",
            code="",
            span=end_span,
            data={},
        )
        self._append_node(start)
        body = self._convert_body(self.function.body)
        self._append_node(end)
        if body:
            self._add_edge(start.id, body[0], "next")
            self._wire_sequence(body, end.id)
        else:
            self._add_edge(start.id, end.id, "next")

        prefix, header, suffix = self._module_parts()
        return PythonFlow(
            schema_version=SCHEMA_VERSION,
            kind=FLOW_KIND,
            source_name=self.source_name,
            source_hash=hashlib.sha256(self.source.encode("utf-8")).hexdigest(),
            entrypoint=self.entrypoint,
            nodes=self.nodes,
            edges=self.edges,
            metadata={
                "body": body,
                "moduleBefore": prefix,
                "moduleAfter": suffix,
                "functionHeader": header,
                "indent": self._body_indent(),
                "async": isinstance(self.function, ast.AsyncFunctionDef),
            },
        )

    def _convert_body(self, statements: Sequence[ast.stmt]) -> list[str]:
        result: list[str] = []
        previous: ast.stmt | None = None
        for statement in statements:
            node_type, data = self._describe_statement(statement)
            leading_trivia = self._leading_trivia(previous, statement)
            if leading_trivia:
                data["leadingTrivia"] = leading_trivia
            code = self._statement_segment(statement)
            node = FlowNode(
                id=self._node_id(node_type, statement),
                type=node_type,
                label=_node_label(node_type, data, code),
                code=code,
                span=_span(statement),
                data=data,
            )
            self._append_node(node)
            result.append(node.id)

            if isinstance(statement, ast.If):
                data["body"] = self._convert_body(statement.body)
                data["orelse"] = self._convert_body(statement.orelse)
            elif isinstance(statement, (ast.For, ast.AsyncFor)):
                data["body"] = self._convert_body(statement.body)
                data["orelse"] = self._convert_body(statement.orelse)
            elif isinstance(statement, ast.While):
                data["body"] = self._convert_body(statement.body)
                data["orelse"] = self._convert_body(statement.orelse)
            elif isinstance(statement, (ast.Try, ast.TryStar)):
                data["body"] = self._convert_body(statement.body)
                handlers: list[dict[str, Any]] = []
                for handler in statement.handlers:
                    handlers.append(
                        {
                            "type": self._expression_segment(handler.type) if handler.type else "",
                            "name": handler.name or "",
                            "body": self._convert_body(handler.body),
                            "star": isinstance(statement, ast.TryStar),
                        }
                    )
                data["handlers"] = handlers
                data["orelse"] = self._convert_body(statement.orelse)
                data["finalbody"] = self._convert_body(statement.finalbody)
            previous = statement
        return result

    def _leading_trivia(
        self,
        previous: ast.stmt | None,
        statement: ast.stmt,
    ) -> list[str]:
        if previous is None or (previous.end_lineno or previous.lineno) >= statement.lineno:
            return []
        base_column = _byte_column_to_character(
            self.lines[statement.lineno - 1], statement.col_offset
        )
        trivia: list[str] = []
        for raw_line in self.lines[previous.end_lineno or previous.lineno : statement.lineno - 1]:
            line = raw_line.rstrip("\r\n")
            removable = min(base_column, len(line) - len(line.lstrip(" \t")))
            trivia.append(line[removable:])
        return trivia

    def _describe_statement(self, statement: ast.stmt) -> tuple[str, dict[str, Any]]:
        call, targets = _statement_call(statement)
        code = self._statement_segment(statement)
        if call is not None:
            call_name = _call_name(call.func)
            root = call_name.split(".", 1)[0]
            if root == "ctx":
                node_type = "ctx-call"
            elif root in self.rpa_aliases or call_name in self.rpa_functions:
                node_type = "rpa-call"
            else:
                node_type = "call"
            return node_type, {
                "statement": code,
                "callName": call_name,
                "arguments": [self._expression_segment(arg) for arg in call.args],
                "keywords": [
                    {
                        "name": keyword.arg,
                        "value": self._expression_segment(keyword.value),
                    }
                    for keyword in call.keywords
                ],
                "assignTargets": targets,
            }
        if isinstance(statement, (ast.Assign, ast.AnnAssign, ast.AugAssign)):
            return "assign", {
                "statement": code,
                "targets": _assignment_targets(statement, self.source),
            }
        if isinstance(statement, ast.If):
            return "if", {"test": self._expression_segment(statement.test)}
        if isinstance(statement, (ast.For, ast.AsyncFor)):
            return "for", {
                "target": self._expression_segment(statement.target),
                "iterator": self._expression_segment(statement.iter),
                "async": isinstance(statement, ast.AsyncFor),
            }
        if isinstance(statement, ast.While):
            return "while", {"test": self._expression_segment(statement.test)}
        if isinstance(statement, (ast.Try, ast.TryStar)):
            return "try", {"star": isinstance(statement, ast.TryStar)}
        if isinstance(statement, ast.Return):
            return "return", {
                "statement": code,
                "value": self._expression_segment(statement.value) if statement.value else "",
            }
        return "raw-code", {"statement": code, "astType": type(statement).__name__}

    def _wire_sequence(
        self,
        node_ids: Sequence[str],
        continuation: str,
        final_kind: str = "next",
    ) -> None:
        for index, node_id in enumerate(node_ids):
            target = node_ids[index + 1] if index + 1 < len(node_ids) else continuation
            kind = "next" if index + 1 < len(node_ids) else final_kind
            self._wire_node(self.node_by_id[node_id], target, kind)

    def _wire_node(self, node: FlowNode, continuation: str, continuation_kind: str) -> None:
        if node.type == "return":
            end_id = next(item.id for item in self.nodes if item.type == "end")
            self._add_edge(node.id, end_id, "return")
            return
        if node.type == "if":
            self._wire_branch(node, "body", "true", continuation, continuation_kind)
            self._wire_branch(node, "orelse", "false", continuation, continuation_kind)
            return
        if node.type in {"for", "while"}:
            body = node.data["body"]
            if body:
                self._add_edge(node.id, body[0], "body")
                self._wire_sequence(body, node.id, "loop-back")
            else:
                self._add_edge(node.id, node.id, "loop-back")
            orelse = node.data["orelse"]
            if orelse:
                self._add_edge(node.id, orelse[0], "exit")
                self._wire_sequence(orelse, continuation, continuation_kind)
            else:
                self._add_edge(node.id, continuation, "exit")
            return
        if node.type == "try":
            self._wire_try(node, continuation, continuation_kind)
            return
        self._add_edge(node.id, continuation, continuation_kind)

    def _wire_branch(
        self,
        node: FlowNode,
        key: str,
        edge_kind: str,
        continuation: str,
        continuation_kind: str,
    ) -> None:
        branch = node.data[key]
        if branch:
            self._add_edge(node.id, branch[0], edge_kind)
            self._wire_sequence(branch, continuation, continuation_kind)
        else:
            self._add_edge(node.id, continuation, edge_kind)

    def _wire_try(self, node: FlowNode, continuation: str, continuation_kind: str) -> None:
        data = node.data
        finalbody = data["finalbody"]
        orelse = data["orelse"]
        final_target = finalbody[0] if finalbody else continuation
        success_target = orelse[0] if orelse else final_target
        body = data["body"]
        if body:
            self._add_edge(node.id, body[0], "try")
            self._wire_sequence(body, success_target)
        else:
            self._add_edge(node.id, success_target, "try")
        for index, handler in enumerate(data["handlers"]):
            handler_body = handler["body"]
            kind = f"except:{index}"
            if handler_body:
                self._add_edge(node.id, handler_body[0], kind)
                self._wire_sequence(handler_body, final_target)
            else:
                self._add_edge(node.id, final_target, kind)
        if orelse:
            self._wire_sequence(orelse, final_target)
        if finalbody:
            self._wire_sequence(finalbody, continuation, continuation_kind)

    def _append_node(self, node: FlowNode) -> None:
        self.nodes.append(node)
        self.node_by_id[node.id] = node

    def _add_edge(self, source: str, target: str, kind: str) -> None:
        key = (source, target, kind)
        if key in self.edge_keys:
            return
        self.edge_keys.add(key)
        edge_seed = f"{source}>{target}:{kind}"
        self.edges.append(
            FlowEdge(
                id=f"edge-{_digest(edge_seed)}",
                source=source,
                target=target,
                kind=kind,
                label=_edge_label(kind),
            )
        )

    def _node_id(self, node_type: str, statement: ast.stmt) -> str:
        semantic = ast.dump(statement, annotate_fields=True, include_attributes=False)
        fingerprint = f"{node_type}:{semantic}"
        occurrence = self.fingerprint_counts.get(fingerprint, 0)
        self.fingerprint_counts[fingerprint] = occurrence + 1
        return f"node-{node_type}-{_digest(fingerprint)}-{occurrence}"

    def _statement_segment(self, node: ast.AST) -> str:
        segment = ast.get_source_segment(self.source, node)
        if segment is None:
            segment = ast.unparse(node)
        if "\n" not in segment:
            return segment
        base_column = _byte_column_to_character(
            self.lines[(node.lineno or 1) - 1], node.col_offset
        )
        parts = segment.splitlines()
        normalized = [parts[0]]
        for line in parts[1:]:
            removable = min(base_column, len(line) - len(line.lstrip(" \t")))
            normalized.append(line[removable:])
        return "\n".join(normalized)

    def _expression_segment(self, node: ast.AST | None) -> str:
        if node is None:
            return ""
        segment = ast.get_source_segment(self.source, node)
        return (segment if segment is not None else ast.unparse(node)).strip()

    def _module_parts(self) -> tuple[str, str, str]:
        decorator_nodes = self.function.decorator_list
        first = min(decorator_nodes, key=lambda item: item.lineno) if decorator_nodes else self.function
        # Decorator AST expressions start after ``@``.  Use the function's base
        # indentation on the decorator line so the marker remains in the header.
        function_start = _offset(self.lines, first.lineno, self.function.col_offset)
        body_start_node = self.function.body[0]
        body_start = _offset(self.lines, body_start_node.lineno, body_start_node.col_offset)
        function_end = _offset(
            self.lines,
            self.function.end_lineno or self.function.lineno,
            self.function.end_col_offset or self.function.col_offset,
        )
        prefix = self.source[:function_start]
        header = self.source[function_start:body_start].rstrip()
        suffix = self.source[function_end:]
        return prefix, header, suffix

    def _body_indent(self) -> str:
        first = self.function.body[0]
        if first.lineno == self.function.lineno:
            function_line = self.lines[self.function.lineno - 1]
            function_column = _byte_column_to_character(
                function_line, self.function.col_offset
            )
            return function_line[:function_column] + "    "
        line = self.lines[first.lineno - 1]
        char_column = _byte_column_to_character(line, first.col_offset)
        indentation = line[:char_column]
        return indentation or "    "


def _render_sequence(
    node_ids: Sequence[str],
    by_id: Mapping[str, FlowNode],
    *,
    level: int,
    indent: str,
) -> list[str]:
    lines: list[str] = []
    for node_id in node_ids:
        node = by_id[node_id]
        trivia = node.data.get("leadingTrivia", [])
        if isinstance(trivia, list):
            prefix = indent * level
            lines.extend(prefix + line if line else "" for line in trivia if isinstance(line, str))
        lines.extend(_render_node(node, by_id, level=level, indent=indent))
    return lines


def _render_node(
    node: FlowNode,
    by_id: Mapping[str, FlowNode],
    *,
    level: int,
    indent: str,
) -> list[str]:
    prefix = indent * level
    if node.type in {"assign", "call", "ctx-call", "rpa-call", "return", "raw-code"}:
        statement = node.code or str(node.data.get("statement", ""))
        if not statement.strip():
            statement = "pass"
        return [prefix + line if line else "" for line in statement.splitlines()]
    if node.type == "if":
        lines = [f"{prefix}if {node.data['test']}:"]
        lines.extend(_render_suite(node.data["body"], by_id, level + 1, indent))
        if node.data["orelse"]:
            lines.append(f"{prefix}else:")
            lines.extend(_render_suite(node.data["orelse"], by_id, level + 1, indent))
        return lines
    if node.type == "for":
        async_prefix = "async " if node.data.get("async") else ""
        lines = [
            f"{prefix}{async_prefix}for {node.data['target']} in {node.data['iterator']}:"
        ]
        lines.extend(_render_suite(node.data["body"], by_id, level + 1, indent))
        if node.data["orelse"]:
            lines.append(f"{prefix}else:")
            lines.extend(_render_suite(node.data["orelse"], by_id, level + 1, indent))
        return lines
    if node.type == "while":
        lines = [f"{prefix}while {node.data['test']}:"]
        lines.extend(_render_suite(node.data["body"], by_id, level + 1, indent))
        if node.data["orelse"]:
            lines.append(f"{prefix}else:")
            lines.extend(_render_suite(node.data["orelse"], by_id, level + 1, indent))
        return lines
    if node.type == "try":
        lines = [f"{prefix}try:"]
        lines.extend(_render_suite(node.data["body"], by_id, level + 1, indent))
        for handler in node.data["handlers"]:
            keyword = "except*" if handler.get("star") else "except"
            type_name = handler.get("type", "")
            name = handler.get("name", "")
            clause = keyword
            if type_name:
                clause += f" {type_name}"
            if name:
                clause += f" as {name}"
            lines.append(f"{prefix}{clause}:")
            lines.extend(_render_suite(handler["body"], by_id, level + 1, indent))
        if node.data["orelse"]:
            lines.append(f"{prefix}else:")
            lines.extend(_render_suite(node.data["orelse"], by_id, level + 1, indent))
        if node.data["finalbody"]:
            lines.append(f"{prefix}finally:")
            lines.extend(_render_suite(node.data["finalbody"], by_id, level + 1, indent))
        return lines
    raise PythonFlowValidationError(f"node {node.id} cannot be rendered: {node.type}")


def _render_suite(
    node_ids: Sequence[str],
    by_id: Mapping[str, FlowNode],
    level: int,
    indent: str,
) -> list[str]:
    rendered = _render_sequence(node_ids, by_id, level=level, indent=indent)
    return rendered or [indent * level + "pass"]


def _statement_call(statement: ast.stmt) -> tuple[ast.Call | None, list[str]]:
    value: ast.AST | None = None
    targets: list[str] = []
    if isinstance(statement, ast.Expr):
        value = statement.value
    elif isinstance(statement, ast.Assign):
        value = statement.value
        targets = [ast.unparse(target) for target in statement.targets]
    elif isinstance(statement, ast.AnnAssign):
        value = statement.value
        targets = [ast.unparse(statement.target)]
    if isinstance(value, ast.Await):
        value = value.value
    return (value, targets) if isinstance(value, ast.Call) else (None, [])


def _assignment_targets(statement: ast.stmt, source: str) -> list[str]:
    if isinstance(statement, ast.Assign):
        nodes: Sequence[ast.AST] = statement.targets
    elif isinstance(statement, (ast.AnnAssign, ast.AugAssign)):
        nodes = [statement.target]
    else:
        return []
    return [ast.get_source_segment(source, node) or ast.unparse(node) for node in nodes]


def _call_name(function: ast.AST) -> str:
    if isinstance(function, ast.Name):
        return function.id
    if isinstance(function, ast.Attribute):
        owner = _call_name(function.value)
        return f"{owner}.{function.attr}" if owner else function.attr
    return ast.unparse(function)


def _collect_rpa_aliases(tree: ast.Module) -> tuple[set[str], set[str]]:
    aliases = {"rpa"}
    functions: set[str] = set()
    for node in tree.body:
        if isinstance(node, ast.Import):
            for alias in node.names:
                if alias.name == "rpa" or alias.name.startswith("rpa."):
                    aliases.add(alias.asname or alias.name.split(".", 1)[0])
        elif isinstance(node, ast.ImportFrom) and node.module and (
            node.module == "rpa" or node.module.startswith("rpa.")
        ):
            for alias in node.names:
                if alias.name != "*":
                    functions.add(alias.asname or alias.name)
    return aliases, functions


def _validate_structured_node(node: FlowNode, node_ids: set[str]) -> None:
    data = node.data
    if node.type == "if":
        _require_string(data, "test", node.id)
        _validate_node_references(_require_list(data, "body", node.id), node_ids, node.id)
        _validate_node_references(_require_list(data, "orelse", node.id), node_ids, node.id)
    elif node.type in {"for", "while"}:
        if node.type == "for":
            _require_string(data, "target", node.id)
            _require_string(data, "iterator", node.id)
        else:
            _require_string(data, "test", node.id)
        _validate_node_references(_require_list(data, "body", node.id), node_ids, node.id)
        _validate_node_references(_require_list(data, "orelse", node.id), node_ids, node.id)
    elif node.type == "try":
        _validate_node_references(_require_list(data, "body", node.id), node_ids, node.id)
        _validate_node_references(_require_list(data, "orelse", node.id), node_ids, node.id)
        _validate_node_references(_require_list(data, "finalbody", node.id), node_ids, node.id)
        handlers = _require_list(data, "handlers", node.id)
        for handler in handlers:
            if not isinstance(handler, Mapping):
                raise PythonFlowValidationError(f"node {node.id} contains an invalid try handler")
            _validate_node_references(
                _require_list(handler, "body", node.id), node_ids, f"{node.id}.handler"
            )


def _validate_node_references(values: Sequence[Any], node_ids: set[str], owner: str) -> None:
    for value in values:
        if not isinstance(value, str) or value not in node_ids:
            raise PythonFlowValidationError(f"{owner} references missing node {value}")


def _validate_span(span: SourceSpan, node_id: str) -> None:
    values = (span.start_line, span.start_column, span.end_line, span.end_column)
    if span.start_line < 1 or span.end_line < span.start_line or any(value < 0 for value in values[1:]):
        raise PythonFlowValidationError(f"node {node_id} has an invalid source span")
    if span.start_line == span.end_line and span.end_column < span.start_column:
        raise PythonFlowValidationError(f"node {node_id} has an invalid source span")


def _require_list(data: Mapping[str, Any], key: str, node_id: str) -> list[Any]:
    value = data.get(key)
    if not isinstance(value, list):
        raise PythonFlowValidationError(f"node {node_id} data.{key} must be an array")
    return value


def _require_string(data: Mapping[str, Any], key: str, node_id: str) -> str:
    value = data.get(key)
    if not isinstance(value, str):
        raise PythonFlowValidationError(f"node {node_id} data.{key} must be a string")
    return value


def _validate_source_name(source_name: str) -> None:
    if not isinstance(source_name, str) or not _SAFE_SOURCE_NAME.fullmatch(source_name):
        raise PythonFlowError(
            "source_name must be a plain .py basename without directories or path separators"
        )


def _request_flow(request: Mapping[str, Any]) -> Mapping[str, Any]:
    if "flow" not in request:
        raise PythonFlowError("operation requires inline flow")
    return _mapping(request["flow"], "flow")


def _load_json(payload: str | bytes) -> Any:
    if not isinstance(payload, (str, bytes)):
        raise TypeError("JSON payload must be str or bytes")
    size = len(payload.encode("utf-8")) if isinstance(payload, str) else len(payload)
    if size > MAX_JSON_BYTES:
        raise PythonFlowError(f"JSON payload exceeds {MAX_JSON_BYTES} bytes")
    return json.loads(payload)


def _mapping(value: Any, name: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise PythonFlowValidationError(f"{name} must be a JSON object")
    return value


def _span(node: ast.AST) -> SourceSpan:
    return SourceSpan(
        start_line=node.lineno,
        start_column=node.col_offset,
        end_line=node.end_lineno or node.lineno,
        end_column=node.end_col_offset or node.col_offset,
    )


def _digest(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()[:12]


def _offset(lines: Sequence[str], line: int, byte_column: int) -> int:
    prefix = sum(len(item) for item in lines[: line - 1])
    return prefix + _byte_column_to_character(lines[line - 1], byte_column)


def _byte_column_to_character(line: str, byte_column: int) -> int:
    encoded = line.encode("utf-8")[:byte_column]
    return len(encoded.decode("utf-8", errors="ignore"))


def _node_label(node_type: str, data: Mapping[str, Any], code: str) -> str:
    if node_type in {"call", "ctx-call", "rpa-call"}:
        return str(data.get("callName") or "Call")
    if node_type == "assign":
        targets = data.get("targets") or []
        return f"Assign {', '.join(targets)}" if targets else "Assign"
    if node_type == "if":
        return f"If {data.get('test', '')}"
    if node_type == "for":
        return f"For {data.get('target', '')}"
    if node_type == "while":
        return f"While {data.get('test', '')}"
    if node_type == "try":
        return "Try"
    if node_type == "return":
        return "Return"
    first_line = code.strip().splitlines()[0] if code.strip() else "Raw code"
    return first_line[:80]


def _edge_label(kind: str) -> str:
    return {
        "true": "True",
        "false": "False",
        "body": "Body",
        "exit": "Exit",
        "loop-back": "Repeat",
        "return": "Return",
        "try": "Try",
    }.get(kind, "Exception" if kind.startswith("except:") else "")


__all__ = [
    "FLOW_KIND",
    "NODE_TYPES",
    "SCHEMA_VERSION",
    "FlowEdge",
    "FlowNode",
    "PythonFlow",
    "PythonFlowError",
    "PythonFlowSyntaxError",
    "PythonFlowValidationError",
    "SourceSpan",
    "flow_from_json",
    "flow_to_json",
    "flow_to_python",
    "handle_json_request",
    "main",
    "python_to_flow",
    "validate_flow",
]


if __name__ == "__main__":
    raise SystemExit(main())
