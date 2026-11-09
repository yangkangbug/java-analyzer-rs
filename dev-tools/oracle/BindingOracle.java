// Copyright 2026 OpenIDEA contributors. SPDX-License-Identifier: Apache-2.0
import com.sun.source.tree.*;
import com.sun.source.util.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import javax.lang.model.element.*;
import javax.tools.*;

/** Development-only compiler comparator. Never generates or executes fixture classes. */
public final class BindingOracle {
    public static void main(String[] args) throws Exception {
        Path root = Path.of(args[0]).toAbsolutePath().normalize();
        List<String> requests = Files.readAllLines(Path.of(args[1]));
        List<Path> files = new ArrayList<>();
        for (String line : requests) {
            String[] c = line.split("\t");
            if (c[0].equals("F")) files.add(root.resolve(c[1]));
        }
        JavaCompiler compiler = ToolProvider.getSystemJavaCompiler();
        DiagnosticCollector<JavaFileObject> diagnostics = new DiagnosticCollector<>();
        try (StandardJavaFileManager fm = compiler.getStandardFileManager(diagnostics, Locale.ROOT, StandardCharsets.UTF_8)) {
            JavacTask task = (JavacTask) compiler.getTask(null, fm, diagnostics,
                List.of("--release", "21", "-proc:none", "-implicit:none", "-encoding", "UTF-8",
                        "-classpath", args[2], "-sourcepath", args.length > 3 ? args[3] : args[2]), null,
                fm.getJavaFileObjectsFromPaths(files));
            List<CompilationUnitTree> units = new ArrayList<>();
            task.parse().forEach(units::add);
            task.analyze();
            for (Diagnostic<?> d : diagnostics.getDiagnostics()) {
                if (d.getKind() == Diagnostic.Kind.ERROR) {
                    System.err.println(d.getCode() + ": " + d.getMessage(Locale.ROOT));
                }
            }
            if (diagnostics.getDiagnostics().stream().anyMatch(d -> d.getKind() == Diagnostic.Kind.ERROR)) {
                System.exit(2);
            }
            Trees trees = Trees.instance(task);
            SourcePositions positions = trees.getSourcePositions();
            Set<String> emitted = new HashSet<>();
            for (CompilationUnitTree unit : units) {
                String path = root.relativize(Path.of(unit.getSourceFile().toUri())).toString().replace('\\', '/');
                String source = unit.getSourceFile().getCharContent(true).toString();
                new TreePathScanner<Void, Void>() {
                    private void inspect(Tree tree, String name) {
                        long end = positions.getEndPosition(unit, tree);
                        if (end < 0) return;
                        int token = (int) end - name.length();
                        int queryByte = source.substring(0, token).getBytes(StandardCharsets.UTF_8).length;
                        for (String request : requests) {
                            String[] c = request.split("\t");
                            if (!c[0].equals("Q") || !c[2].equals(path) || Integer.parseInt(c[3]) != queryByte) continue;
                            if (!emitted.add(c[1])) throw new IllegalStateException("duplicate query " + c[1]);
                            Element element = trees.getElement(getCurrentPath());
                            TreePath declaration = element == null ? null : trees.getPath(element);
                            if (declaration == null) throw new IllegalStateException("no source declaration for " + c[1]);
                            CompilationUnitTree targetUnit = declaration.getCompilationUnit();
                            Tree target = declaration.getLeaf();
                            String targetSource;
                            try { targetSource = targetUnit.getSourceFile().getCharContent(true).toString(); }
                            catch (java.io.IOException e) { throw new RuntimeException(e); }
                            int start = Math.toIntExact(positions.getStartPosition(targetUnit, target));
                            int finish = Math.toIntExact(positions.getEndPosition(targetUnit, target));
                            String targetPath = root.relativize(Path.of(targetUnit.getSourceFile().toUri())).toString().replace('\\', '/');
                            Element owner = element instanceof TypeElement ? element : element.getEnclosingElement();
                            while (owner != null && !(owner instanceof TypeElement)) owner = owner.getEnclosingElement();
                            System.out.printf("%s\t%s\t%s\t%s\t%s\t%d\t%d%n", c[1], targetPath,
                                element.getKind(), owner == null ? "" : ((TypeElement) owner).getQualifiedName(),
                                element.getSimpleName(), targetSource.substring(0, start).getBytes(StandardCharsets.UTF_8).length,
                                targetSource.substring(0, finish).getBytes(StandardCharsets.UTF_8).length);
                        }
                    }
                    @Override public Void visitIdentifier(IdentifierTree tree, Void p) {
                        inspect(tree, tree.getName().toString()); return super.visitIdentifier(tree, p);
                    }
                    @Override public Void visitMemberSelect(MemberSelectTree tree, Void p) {
                        inspect(tree, tree.getIdentifier().toString()); return super.visitMemberSelect(tree, p);
                    }
                }.scan(unit, null);
            }
            long count = requests.stream().filter(s -> s.startsWith("Q\t")).count();
            if (emitted.size() != count) throw new IllegalStateException("unvisited query positions");
        }
    }
}
