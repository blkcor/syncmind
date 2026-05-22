## ADDED Requirements

### Requirement: Default tree-sitter language coverage
The code chunker SHALL support tree-sitter AST chunking by default for Rust, Python, JavaScript, TypeScript, Go, Java, C, C++, C#, Ruby, PHP, Swift, and Kotlin source files.

#### Scenario: Existing language regression coverage
- **WHEN** a `.rs`, `.py`, `.js`, `.jsx`, `.ts`, `.tsx`, or `.go` file is indexed
- **THEN** the code chunker SHALL continue to route the file to its tree-sitter parser
- **AND** produce chunks at language-aware syntactic boundaries

#### Scenario: Added language parser coverage
- **WHEN** a `.java`, `.c`, `.h`, `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hh`, `.hxx`, `.cs`, `.rb`, `.php`, `.swift`, `.kt`, or `.kts` file is indexed
- **THEN** the code chunker SHALL route the file to the matching tree-sitter parser
- **AND** produce chunks at language-aware syntactic boundaries

### Requirement: Language-specific syntactic chunk boundaries
For each supported language, the code chunker SHALL define a language-specific set of AST node types that represent useful top-level or declaration-level chunk boundaries.

#### Scenario: Java declaration boundaries
- **WHEN** a Java file is parsed
- **THEN** chunks SHALL be extracted around class, interface, enum, constructor, method, and field declaration boundaries where available

#### Scenario: C and C++ declaration boundaries
- **WHEN** a C or C++ file is parsed
- **THEN** chunks SHALL be extracted around function, struct, union, enum, class, namespace, method, and declaration boundaries where available for the language

#### Scenario: C# declaration boundaries
- **WHEN** a C# file is parsed
- **THEN** chunks SHALL be extracted around namespace, class, struct, interface, enum, constructor, method, property, and field declaration boundaries where available

#### Scenario: Dynamic and mobile language boundaries
- **WHEN** a Ruby, PHP, Swift, Kotlin, or JavaScript/TypeScript file is parsed
- **THEN** chunks SHALL be extracted around module, class, struct, enum, function, method, property, and top-level declaration boundaries where available for the language

### Requirement: Unsupported and failed parsers use fallback chunking
The code chunker SHALL delegate to `FallbackChunker` when a file extension is unsupported, when a tree-sitter grammar is unavailable, or when parsing fails for a supported language.

#### Scenario: Unsupported language fallback
- **WHEN** a code-like file extension is not mapped to a supported tree-sitter language
- **THEN** the code chunker SHALL delegate to `FallbackChunker`
- **AND** indexing SHALL continue without error

#### Scenario: Parser failure fallback
- **WHEN** a supported-language parser fails to initialize or parse a source file
- **THEN** the code chunker SHALL record a warning with the language and file path
- **AND** delegate that file to `FallbackChunker`
- **AND** indexing SHALL continue without error

### Requirement: Fixture-based coverage verification
The test suite SHALL include representative source fixtures for every default supported tree-sitter language and for at least one unsupported language fallback case.

#### Scenario: Added language fixture tests
- **WHEN** chunker tests run
- **THEN** fixtures for Java, C, C++, C#, Ruby, PHP, Swift, and Kotlin SHALL assert that language-aware chunks are produced
- **AND** each fixture SHALL verify at least one expected declaration or function boundary

#### Scenario: Existing language regression tests
- **WHEN** chunker tests run
- **THEN** fixtures for Rust, Python, JavaScript, TypeScript, and Go SHALL continue to pass through tree-sitter chunking
- **AND** they SHALL NOT regress to `FallbackChunker`
