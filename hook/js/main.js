if (globalThis.Deno) {
   const opName = "op_xyz";
   const ops = Deno.core.ops;
   function encode(str) {
      const charCodes = str.split("").map((c) => c.charCodeAt(0));
      const ui8 = new Uint8Array(charCodes);
      return ui8;
   }
   function decodeAscii(ui8) {
      let out = "";
      if (!ui8) {
         return out;
      }
      for (let i = 0; i < ui8.length; i++) {
         out += String.fromCharCode(ui8[i]);
      }
      return out;
   }
   const asdf = "asdf";
   const msg = encode("calling a rust function from deno");

   if (Deno.core.op_xyz) {
      const resUi8 = Deno.core.ops.op_xyz(msg);
      console.log(resUi8);
   }

   console.log("WHAT asdfd asdfds afdsaf sf");
}

const proxySymbol = Symbol("Proxy");

globalThis.devtoolsFormatters = [{
   header: function(obj, config) {
      if (typeof obj === 'object' && obj[proxySymbol]) {
         return ['object', { object: { ...obj } }];
      } else {
         return null;
      }
   },
   hasBody: function() {
      return false;
   },
}]

console.log({});



let target = {
   message1: "hello",
   message2: "everyone",
};

let handler1 = {
   get(target, prop, receiver) {
      if (prop === proxySymbol) {
         return true;
      }
      return "world";
   }
};

let proxy1 = new Proxy(target, handler1);


console.log(proxy1);


globalThis.setInterval = (callback, delay = 0) => Deno.core.queueUserTimer(
   Deno.core.getTimerDepth() + 1,
   true,
   delay,
   callback
)



setInterval(() => console.log('asdf'), 1000);
setInterval(() => console.log('some time long in the future to keep the VM alive'), 1000000);

Deno.core.ops.op_ue_hook('/Script/Engine.KismetSystemLibrary:PrintString', () => { console.log('hooked from JS!!') })

//debugger;

//throw new Error(`result: ${decodeAscii(resUi8)}`);
//import {
//  add,
//  multiply,
//} from "https://x.nest.land/ramda@0.27.0/source/index.js";
//console.log(add);
