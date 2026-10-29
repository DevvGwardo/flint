import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:hooter_genui/bridge/framing.dart';
import 'package:hooter_genui/bridge/protocol.dart';

void main() {
  test('four-byte big-endian header uses UTF8 length', () {
    final frame = encodeFrame({'x': '😀'});
    expect(
      ByteData.sublistView(frame).getUint32(0, Endian.big),
      utf8.encode('{"x":"😀"}').length,
    );
  });

  test('all split boundaries and byte-by-byte delivery are correct', () {
    final frame = encodeFrame({'text': 'é😀'});
    for (var split = 0; split <= frame.length; split++) {
      final received = <Object?>[];
      final decoder = FrameDecoder(received.add);
      decoder.add(frame.sublist(0, split));
      decoder.add(frame.sublist(split));
      decoder.finish();
      expect(received, [
        {'text': 'é😀'},
      ]);
    }
    final received = <Object?>[];
    final decoder = FrameDecoder(received.add);
    for (final byte in frame) {
      decoder.add([byte]);
    }
    expect(received, [
      {'text': 'é😀'},
    ]);
    expect(decoder.hasPartialFrame, false);
  });

  test('multiple frames in one chunk require no unbounded queue', () {
    final received = <Object?>[];
    final decoder = FrameDecoder(received.add);
    decoder.add([
      for (var i = 0; i < 100; i++) ...encodeFrame({'i': i}),
    ]);
    expect(received.length, 100);
    expect(decoder.completedFrames, 100);
    decoder.finish();
  });

  test('zero and oversize length reject before allocating payload', () {
    for (final size in [0, maxFrameBytes + 1, 0xffffffff]) {
      final header = Uint8List(4);
      ByteData.sublistView(header).setUint32(0, size, Endian.big);
      final decoder = FrameDecoder((_) => fail('must not decode'));
      expect(() => decoder.add(header), throwsA(isA<ProtocolFailure>()));
      expect(decoder.hasPartialFrame, false);
    }
  });

  test('EOF rejects each truncated header or payload', () {
    final frame = encodeFrame({'ok': true});
    for (var length = 1; length < frame.length; length++) {
      final decoder = FrameDecoder((_) => fail('must not decode'));
      decoder.add(frame.sublist(0, length));
      expect(decoder.finish, throwsA(isA<ProtocolFailure>()));
    }
    FrameDecoder((_) {}).finish();
  });

  test('fatal decoder cannot deliver subsequent frames', () {
    final received = <Object?>[];
    final decoder = FrameDecoder(received.add);
    expect(() => decoder.add([0, 0, 0, 0]), throwsA(isA<ProtocolFailure>()));
    decoder.add(encodeFrame({'no': true}));
    expect(received, isEmpty);
  });

  test('a callback can close decoder mid-chunk', () {
    var count = 0;
    late FrameDecoder decoder;
    decoder = FrameDecoder((_) {
      count++;
      decoder.close();
    });
    decoder.add([
      ...encodeFrame({'one': 1}),
      ...encodeFrame({'two': 2}),
    ]);
    expect(count, 1);
  });

  test('maximum frame boundary is inclusive and encoder is bounded', () {
    final payload = {'x': 'a' * (maxFrameBytes - 8)};
    final frame = encodeFrame(payload);
    expect(frame.length, maxFrameBytes + 4);
    final received = <Object?>[];
    FrameDecoder(received.add).add(frame);
    expect(received.single, payload);
    expect(
      () => encodeFrame({'x': 'a' * (maxFrameBytes - 7)}),
      throwsA(isA<ProtocolFailure>()),
    );
  });
}
