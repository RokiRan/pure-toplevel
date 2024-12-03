// 顶层函数调用
foo();
bar();

// 成员函数调用
console.log('test');

// 嵌套函数调用
function test() {
    foo();
    bar();
}

// 箭头函数中的调用
const arrow = () => {
    foo();
    bar();
};
