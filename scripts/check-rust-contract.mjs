import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const projectRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const typesPath = path.join(projectRoot, "src/lib/types.ts");
const fixturePath = path.join(
  projectRoot,
  "src/lib/__rust_contract_fixture__.ts",
);
const contract = readFileSync(typesPath, "utf8");
const payload = JSON.parse(readFileSync(0, "utf8"));

function verifyContract(source, { snapshot, actions }) {
  const ast = ts.createSourceFile(
    typesPath,
    source,
    ts.ScriptTarget.Latest,
    true,
  );
  const actionType = ast.statements.find(
    (node) =>
      ts.isTypeAliasDeclaration(node) && node.name.text === "WorkspaceAction",
  );
  assert(
    actionType && ts.isUnionTypeNode(actionType.type),
    "Missing WorkspaceAction union",
  );
  const tags = actionType.type.types.map((variant) => {
    assert(ts.isTypeLiteralNode(variant), "Expected an object action variant");
    const tag = variant.members.find(
      (member) =>
        ts.isPropertySignature(member) && member.name?.getText(ast) === "type",
    );
    assert(
      tag?.type &&
        ts.isLiteralTypeNode(tag.type) &&
        ts.isStringLiteral(tag.type.literal),
      "Every action needs a string literal type tag",
    );
    return tag.type.literal.text;
  });
  assert.equal(
    new Set(tags).size,
    tags.length,
    "Duplicate action tags in TypeScript",
  );
  assert.deepEqual(
    actions.map((action) => action.type).sort(),
    tags.sort(),
    "Rust fixtures must cover every TypeScript action exactly once",
  );

  const snapshotType = ast.statements.find(
    (node) =>
      ts.isInterfaceDeclaration(node) && node.name.text === "WorkspaceSnapshot",
  );
  assert(snapshotType, "Missing WorkspaceSnapshot interface");
  const fields = snapshotType.members.map((member) => {
    assert(ts.isPropertySignature(member), "Expected a snapshot property");
    return member.name.getText(ast);
  });
  const requiredFields = snapshotType.members
    .filter((member) => !member.questionToken)
    .map((member) => member.name.getText(ast));
  assert.ok(
    requiredFields.every((field) => Object.hasOwn(snapshot, field)),
    "Snapshot field mismatch: required property missing",
  );
  assert.ok(
    Object.keys(snapshot).every((field) => fields.includes(field)),
    "Snapshot field mismatch: unexpected property",
  );

  // Contextually type-check the actual JSON serialized by Rust against the shared
  // interfaces, including nested values, camelCase keys, and discriminated unions.
  const fixture = `import type { WorkspaceAction, WorkspaceSnapshot } from "./types.js";
const snapshot: WorkspaceSnapshot = ${JSON.stringify(snapshot)};
const actions: WorkspaceAction[] = ${JSON.stringify(actions)};
`;
  const options = {
    strict: true,
    noEmit: true,
    skipLibCheck: true,
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.NodeNext,
    moduleResolution: ts.ModuleResolutionKind.NodeNext,
    types: [],
  };
  const host = ts.createCompilerHost(options);
  const readFile = host.readFile.bind(host);
  const fileExists = host.fileExists.bind(host);
  host.readFile = (name) =>
    name === fixturePath
      ? fixture
      : name === typesPath
        ? source
        : readFile(name);
  host.fileExists = (name) => name === fixturePath || fileExists(name);
  const program = ts.createProgram([fixturePath], options, host);
  const diagnostics = ts.getPreEmitDiagnostics(program);
  if (diagnostics.length) {
    throw new Error(
      ts.formatDiagnosticsWithColorAndContext(diagnostics, {
        getCanonicalFileName: (name) => name,
        getCurrentDirectory: () => projectRoot,
        getNewLine: () => "\n",
      }),
    );
  }
}

verifyContract(contract, payload);
// Keep the validator itself honest: formatting is irrelevant; real drift fails.
verifyContract(contract.replaceAll('"', "'"), payload);
assert.throws(
  () =>
    verifyContract(contract, {
      ...payload,
      actions: payload.actions.filter((action) => action.type !== "cancel"),
    }),
  /every TypeScript action/,
);
assert.throws(
  () =>
    verifyContract(contract, {
      ...payload,
      snapshot: { ...payload.snapshot, unexpectedField: true },
    }),
  /Snapshot field mismatch/,
);
assert.throws(
  () =>
    verifyContract(contract, {
      ...payload,
      actions: payload.actions.map((action) =>
        action.type === "set_overlap"
          ? { ...action, allow: "invalid" }
          : action,
      ),
    }),
  /not assignable to type 'boolean'/,
);
console.log(
  `Verified ${payload.actions.length} action variants and ${Object.keys(payload.snapshot).length} snapshot fields against TypeScript.`,
);
