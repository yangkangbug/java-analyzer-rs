// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package client;
import api.Target;
import dep.Token;
// UTF-8 / UTF-16 distinction: 🪴 中文.
public class Use {
 void run(Target target, String text, Token token) {
  target./*Q:binary_string*/take(text);
  target./*Q:binary_token*/token(token);
  target./*Q:binary_widen_refusal*/widen(token);
 }
}
