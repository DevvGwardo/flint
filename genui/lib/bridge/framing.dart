import 'dart:convert';
import 'dart:typed_data';

import 'protocol.dart';

Uint8List encodeFrame(Map<String, Object?> value) {
  final payload = utf8.encode(jsonEncode(value));
  if (payload.isEmpty || payload.length > maxFrameBytes) {
    throw const ProtocolFailure(BridgeError.malformedFrame);
  }
  final frame = Uint8List(payload.length + 4);
  ByteData.sublistView(frame).setUint32(0, payload.length, Endian.big);
  frame.setRange(4, frame.length, payload);
  return frame;
}

/// Stores at most one header and one bounded payload, even when a socket
/// chunk contains many frames. Processing is synchronous; no inbound queue.
final class FrameDecoder {
  FrameDecoder(this.onFrame);
  final void Function(Object?) onFrame;
  final _header = Uint8List(4);
  var _headerUsed = 0;
  Uint8List? _payload;
  var _payloadUsed = 0;
  var _closed = false;

  bool get hasPartialFrame => _headerUsed != 0 || _payload != null;
  int completedFrames = 0;

  void add(List<int> bytes) {
    if (_closed) return;
    var offset = 0;
    try {
      while (offset < bytes.length && !_closed) {
        if (_payload == null) {
          final count = (4 - _headerUsed).clamp(0, bytes.length - offset);
          _header.setRange(_headerUsed, _headerUsed + count, bytes, offset);
          offset += count;
          _headerUsed += count;
          if (_headerUsed != 4) continue;
          final size = ByteData.sublistView(_header).getUint32(0, Endian.big);
          if (size == 0 || size > maxFrameBytes) {
            throw const ProtocolFailure(BridgeError.malformedFrame);
          }
          _payload = Uint8List(size);
          _payloadUsed = 0;
        }
        final payload = _payload!;
        final count = (payload.length - _payloadUsed).clamp(
          0,
          bytes.length - offset,
        );
        payload.setRange(_payloadUsed, _payloadUsed + count, bytes, offset);
        offset += count;
        _payloadUsed += count;
        if (_payloadUsed == payload.length) {
          final decoded = decodeStrictJson(payload);
          _payload = null;
          _headerUsed = 0;
          _payloadUsed = 0;
          completedFrames++;
          onFrame(decoded);
        }
      }
    } catch (_) {
      close();
      rethrow;
    }
  }

  void finish() {
    final truncated = hasPartialFrame;
    close();
    if (truncated) throw const ProtocolFailure(BridgeError.malformedFrame);
  }

  void close() {
    _closed = true;
    _payload = null;
    _payloadUsed = 0;
    _headerUsed = 0;
  }
}
