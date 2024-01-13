function main() {
  let arr1 = require("./test.json");
  let arr2 = require("./test2.json");

  for (let i = 0; i < Math.max(arr1.length); i++) {
    if (arr1[i] !== arr2[i]) {
      console.log("not equal", i);
      console.log(arr1[i]);
      console.log(arr2[i]);
    }
  }
}

main();
