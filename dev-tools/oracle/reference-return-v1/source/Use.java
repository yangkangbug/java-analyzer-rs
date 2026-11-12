// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
package caller;
import api.Factory;
import api.Sink;
import shadow.Product;
// Supplementary position prefix: 🪴 中文.
public class Use {
 int unknown() { return 1; }
 void run(Factory f, Sink sink, Product decoy) {
  f.make(1)./*Q:return_direct*/value();
  (f.make(1))./*Q:return_parens*/value();
  f.array()[0]./*Q:return_array*/value();
  f.generic()./*Q:return_generic*/value();
  f.make(1).next()./*Q:return_deep*/value();
  f.make(unknown())./*Q:return_unknown_inner*/value();
  sink./*Q:return_argument*/consume(f.make(1));
 }
}
