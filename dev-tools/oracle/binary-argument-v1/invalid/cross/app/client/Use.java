// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package client;
import api.Target;
import dep.Token;
public class Use {
 void run(Target target, Token token, byte b) {
  target./*Q:cross_unknown_competitor*/cross(token, b);
 }
}
