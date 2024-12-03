# Babel Pure Top-Level Functions Plugin

A high-performance Babel plugin written in Rust to annotate top-level function calls with `/*#__PURE__*/` comment for better tree-shaking and optimization.

## Requirements

### Rust
- Rust Edition: 2021
- Minimum Rust Version: 1.67.0
- Recommended Rust Toolchain: Stable (latest)

### Node.js
- Minimum Version: 16.x
- Recommended Version: 18.x or 20.x

### Package Managers
- npm
- yarn
- pnpm (Recommended)

## Installation

Using pnpm (recommended):
```bash
pnpm add -D babel-plugin-pure-toplevel
```

Using npm:
```bash
npm install --save-dev babel-plugin-pure-toplevel
```

Using yarn:
```bash
yarn add -D babel-plugin-pure-toplevel
```

## Usage

### Babel Configuration

#### `.babelrc`
```json
{
  "plugins": [
    "babel-plugin-pure-toplevel"
  ]
}
```

#### Programmatic Usage
```javascript
import babel from '@babel/core';
import pureTopLevelPlugin from 'babel-plugin-pure-toplevel';

const result = babel.transform(code, {
  plugins: [pureTopLevelPlugin()]
});
```

## Example

### Input
```javascript
function test() {
  Object.create({});
  new Date();
}
```

### Output
```javascript
function test() {
  /*#__PURE__*/Object.create({});
  /*#__PURE__*/new Date();
}
```

## Features

- ✅ Annotate top-level function calls
- ✅ Skip TypeScript helper functions
- ✅ Skip function calls with arguments
- ✅ Support for `CallExpression` and `NewExpression`
- 🚀 High-performance Rust implementation

## Development

### Build Plugin

```bash
# Build for release
pnpm build

# Build for debug
pnpm build:debug
```

### Run Tests

```bash
# Rust tests
pnpm test

# Babel integration tests (to be implemented)
pnpm test:babel
```

## Performance

This plugin is implemented in Rust, providing near-native performance for AST transformations.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

MIT License

## Acknowledgements

Inspired by Angular's pure annotation strategy and leveraging the power of Rust and SWC.
