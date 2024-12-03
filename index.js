const { createPlugin } = require('./pure-toplevel');

module.exports = function() {
  return {
    name: 'babel-plugin-pure-toplevel',
    visitor: {
      CallExpression(path) {
        // 如果不是顶层调用，跳过
        if (path.getFunctionParent()) {
          return;
        }

        // 调用 Rust 插件的转换逻辑
        const result = createPlugin(path.node);
        
        // 如果返回了修改，应用修改
        if (result) {
          path.addComment('leading', '#__PURE__');
        }
      },
      NewExpression(path) {
        // 如果不是顶层调用，跳过
        if (!path.getFunctionParent()) {
          const result = createPlugin(path.node);
          
          if (result) {
            path.addComment('leading', '#__PURE__');
          }
        }
      }
    }
  };
};
