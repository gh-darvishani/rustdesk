// ChalakDesk: phone-number ID prompt and the remote banner on the home page.
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_hbb/common.dart';
import 'package:flutter_hbb/models/platform_model.dart';
import 'package:http/http.dart' as http;
import 'package:url_launcher/url_launcher.dart';

// Temporary domain until chalak-desk.ir is approved.
const kChalakSite = 'https://desk.chalak-local.ir';
const kChalakBannerUrl = '$kChalakSite/api/banner';

final _mobileRegExp = RegExp(r'^09\d{9}$');
// Phone, or phone + one digit 2-9 for a customer's additional computers.
final _phoneRegExp = RegExp(r'^09\d{9}[2-9]?$');

bool isChalakPhoneId(String id) => _phoneRegExp.hasMatch(id);

bool _phoneDialogShown = false;

/// Asks the customer for their mobile number until the ID has been changed to it.
Future<void> checkChalakPhoneId() async {
  if (_phoneDialogShown) return;
  final id = await bind.mainGetMyId();
  if (id.isEmpty || isChalakPhoneId(id)) return;
  _phoneDialogShown = true;
  _showPhoneDialog();
}

String _toEnglishDigits(String s) {
  const fa = '۰۱۲۳۴۵۶۷۸۹';
  const ar = '٠١٢٣٤٥٦٧٨٩';
  final b = StringBuffer();
  for (final ch in s.split('')) {
    var i = fa.indexOf(ch);
    if (i < 0) i = ar.indexOf(ch);
    b.write(i >= 0 ? '$i' : ch);
  }
  return b.toString();
}

void _showPhoneDialog() {
  final controller = TextEditingController();
  var msg = '';
  var isInProgress = false;

  gFFI.dialogManager.show((setState, close, context) {
    submit() async {
      if (isInProgress) return;
      final phone = _toEnglishDigits(controller.text.trim());
      if (!_mobileRegExp.hasMatch(phone)) {
        setState(() => msg = 'شماره موبایل را به شکل ۰۹۱۲۱۲۳۴۵۶۷ وارد کنید');
        return;
      }
      setState(() {
        msg = '';
        isInProgress = true;
      });
      // The same phone may be used on several computers: the first one gets the plain
      // number, later ones get the number plus 2, 3, ... (the server rejects taken IDs).
      var status = '';
      for (final suffix in ['', '2', '3', '4', '5', '6', '7', '8', '9']) {
        bind.mainChangeId(newId: phone + suffix);
        status = await bind.mainGetAsyncStatus();
        while (status == ' ') {
          await Future.delayed(const Duration(milliseconds: 100));
          status = await bind.mainGetAsyncStatus();
        }
        if (status.isEmpty) {
          _phoneDialogShown = false;
          close();
          return;
        }
        // "Not available" / "Unknown error" mean the ID belongs to another computer.
        if (status != 'Not available' && status != 'Unknown error') break;
        await Future.delayed(const Duration(seconds: 1));
      }
      setState(() {
        isInProgress = false;
        msg = status == 'Not available' || status == 'Unknown error'
            ? 'این شماره روی کامپیوترهای زیادی ثبت شده است. با پشتیبانی تماس بگیرید.'
            : 'ثبت شماره انجام نشد. اتصال اینترنت را بررسی کنید و دوباره تلاش کنید.';
      });
    }

    return CustomAlertDialog(
      title: const Text('شماره موبایل شما'),
      content: Directionality(
        textDirection: TextDirection.rtl,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            const Text(
                'لطفاً شماره موبایل خود را وارد کنید. پشتیبان‌های چالاک با همین شماره به کامپیوتر شما وصل می‌شوند.'),
            const SizedBox(height: 12),
            TextField(
              controller: controller,
              autofocus: true,
              keyboardType: TextInputType.phone,
              textDirection: TextDirection.ltr,
              inputFormatters: [LengthLimitingTextInputFormatter(11)],
              decoration: InputDecoration(
                hintText: '09121234567',
                errorText: msg.isEmpty ? null : msg,
                errorMaxLines: 3,
              ),
              onChanged: (_) {
                if (msg.isNotEmpty) setState(() => msg = '');
              },
              onSubmitted: (_) => submit(),
            ),
            const SizedBox(height: 8),
            if (isInProgress) const LinearProgressIndicator(),
          ],
        ),
      ),
      actions: [
        dialogButton('ثبت', onPressed: submit),
      ],
      onSubmit: submit,
    );
  });
}

/// Banner image and link served by chalak-desk.ir as
/// `{"image": "https://...", "link": "https://..."}`. Hidden when unavailable.
class ChalakBanner extends StatefulWidget {
  const ChalakBanner({Key? key}) : super(key: key);

  @override
  State<ChalakBanner> createState() => _ChalakBannerState();
}

class _ChalakBannerState extends State<ChalakBanner> {
  String _image = '';
  String _link = '';

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final resp = await http
          .get(Uri.parse(kChalakBannerUrl))
          .timeout(const Duration(seconds: 10));
      if (resp.statusCode != 200) return;
      final data = jsonDecode(utf8.decode(resp.bodyBytes));
      final image = (data['image'] ?? '').toString();
      final link = (data['link'] ?? '').toString();
      if (image.startsWith('http') && mounted) {
        setState(() {
          _image = image;
          _link = link;
        });
      }
    } catch (e) {
      debugPrint('chalak banner: $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_image.isEmpty) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 10, 20, 0),
      child: MouseRegion(
        cursor: _link.isEmpty ? MouseCursor.defer : SystemMouseCursors.click,
        child: GestureDetector(
          onTap: _link.startsWith('http')
              ? () => launchUrl(Uri.parse(_link))
              : null,
          child: ClipRRect(
            borderRadius: BorderRadius.circular(8),
            child: Image.network(
              _image,
              fit: BoxFit.fitWidth,
              errorBuilder: (_, __, ___) => const SizedBox.shrink(),
            ),
          ),
        ),
      ),
    );
  }
}

const kChalakVersionUrl = '$kChalakSite/api/version';

/// Compares dotted versions like 1.4.9 and 1.4.10; returns true if [a] > [b].
bool _isNewerVersion(String a, String b) {
  List<int> parse(String v) => v
      .split(RegExp(r'[.\-]'))
      .map((x) => int.tryParse(x) ?? 0)
      .toList();
  final pa = parse(a), pb = parse(b);
  for (var i = 0; i < pa.length || i < pb.length; i++) {
    final x = i < pa.length ? pa[i] : 0;
    final y = i < pb.length ? pb[i] : 0;
    if (x != y) return x > y;
  }
  return false;
}

/// New-version notice. chalak-desk.ir/api/version returns
/// `{"version": "1.4.10", "url": "https://.../ChalakDesk.msi"}`. Hidden when up to date.
class ChalakUpdateCard extends StatefulWidget {
  const ChalakUpdateCard({Key? key}) : super(key: key);

  @override
  State<ChalakUpdateCard> createState() => _ChalakUpdateCardState();
}

class _ChalakUpdateCardState extends State<ChalakUpdateCard> {
  String _version = '';
  String _url = '';

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final resp = await http
          .get(Uri.parse(kChalakVersionUrl))
          .timeout(const Duration(seconds: 10));
      if (resp.statusCode != 200) return;
      final data = jsonDecode(utf8.decode(resp.bodyBytes));
      final latest = (data['version'] ?? '').toString().trim();
      final url = (data['url'] ?? '').toString();
      final current = await bind.mainGetVersion();
      if (latest.isNotEmpty &&
          url.startsWith('http') &&
          _isNewerVersion(latest, current) &&
          mounted) {
        setState(() {
          _version = latest;
          _url = url;
        });
      }
    } catch (e) {
      debugPrint('chalak version: $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_version.isEmpty) return const SizedBox.shrink();
    return Container(
      margin: const EdgeInsets.fromLTRB(20, 10, 20, 0),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: const Color(0xFF0A9471),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Directionality(
        textDirection: TextDirection.rtl,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text('نسخه جدید چالاک‌دسک ($_version) آماده است',
                style: const TextStyle(color: Colors.white, fontSize: 14)),
            const SizedBox(height: 8),
            OutlinedButton(
              style: OutlinedButton.styleFrom(
                  foregroundColor: Colors.white,
                  side: const BorderSide(color: Colors.white)),
              onPressed: () => launchUrl(Uri.parse(_url)),
              child: const Text('دانلود نسخه جدید'),
            ),
          ],
        ),
      ),
    );
  }
}
