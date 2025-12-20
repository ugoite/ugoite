// runner.js
// Multiplexed Stdio Protocol

const STDIN = 0;
const STDOUT = 1;
const STDERR = 2;

function readExact(fd, length) {
  const buffer = new Uint8Array(length);
  let offset = 0;
  while (offset < length) {
    const read = Javy.IO.readSync(fd, buffer.subarray(offset));
    if (read === 0) throw new Error("Unexpected EOF");
    offset += read;
  }
  return buffer;
}

function writeAll(fd, data) {
  Javy.IO.writeSync(fd, data);
}

function readInt32(fd) {
  const buffer = readExact(fd, 4);
  const view = new DataView(buffer.buffer);
  return view.getInt32(0, false); // Big Endian
}

function writeInt32(fd, val) {
  const buffer = new Uint8Array(4);
  const view = new DataView(buffer.buffer);
  view.setInt32(0, val, false); // Big Endian
  writeAll(fd, buffer);
}

// Host Call Implementation
globalThis.host = {
  call: (method, path, body) => {
    const payload = JSON.stringify({ method, path, body });
    const encoder = new TextEncoder();
    const bytes = encoder.encode(payload);
    
    // Header: Magic + Length
    const magic = new TextEncoder().encode("\0HOST\0");
    writeAll(STDOUT, magic);
    writeInt32(STDOUT, bytes.length);
    writeAll(STDOUT, bytes);
    
    // Read Response
    const respLen = readInt32(STDIN);
    const respBytes = readExact(STDIN, respLen);
    const respString = new TextDecoder().decode(respBytes);
    
    return JSON.parse(respString);
  }
};

// Main Loop
try {
  // 1. Read User Code Length
  const codeLen = readInt32(STDIN);
  const codeBytes = readExact(STDIN, codeLen);
  const userCode = new TextDecoder().decode(codeBytes);
  
  // 2. Execute
  const func = new Function(userCode);
  const result = func();
  
  let output;
  if (result === undefined) {
      output = "undefined";
  } else {
      output = JSON.stringify(result);
  }
  
  // 3. Write Result
  const magic = new TextEncoder().encode("\0RSLT\0");
  writeAll(STDOUT, magic);
  const outBytes = new TextEncoder().encode(output);
  writeInt32(STDOUT, outBytes.length);
  writeAll(STDOUT, outBytes);
  
} catch (e) {
  const magic = new TextEncoder().encode("\0ERRR\0");
  writeAll(STDOUT, magic);
  const errBytes = new TextEncoder().encode(e.toString());
  writeInt32(STDOUT, errBytes.length);
  writeAll(STDOUT, errBytes);
}
