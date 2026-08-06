// Command gtx2 recursively extracts gophertunnel packet wire operations.
// It deliberately uses only go/ast: the source need not type-check and no modules are downloaded.
package main

import (
	"encoding/json"
	"fmt"
	"go/ast"
	"go/parser"
	"go/printer"
	"go/token"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

const depthLimit = 40

type Operation struct {
	Kind     string      `json:"kind"`
	Field    string      `json:"field,omitempty"`
	Op       string      `json:"op,omitempty"`
	Prefix   string      `json:"prefix,omitempty"`
	Encoding string      `json:"encoding,omitempty"`
	Presence string      `json:"presence,omitempty"`
	Length   int         `json:"length,omitempty"`
	Element  []Operation `json:"element,omitempty"`
	Value    []Operation `json:"value,omitempty"`
	TypeName string      `json:"type_name,omitempty"`
	Reason   string      `json:"reason,omitempty"`
	Site     string      `json:"site,omitempty"`
}

type Packet struct {
	ID         int         `json:"id"`
	Name       string      `json:"name"`
	Operations []Operation `json:"operations"`
}

type Manifest struct {
	SchemaVersion    int      `json:"schema_version"`
	Source           string   `json:"source"`
	MinecraftVersion string   `json:"minecraft_version"`
	ProtocolVersion  int      `json:"protocol_version"`
	Packets          []Packet `json:"packets"`
}

type Diagnostic struct {
	Packet string `json:"packet,omitempty"`
	Type   string `json:"type,omitempty"`
	Field  string `json:"field,omitempty"`
	File   string `json:"file"`
	Line   int    `json:"line"`
	Reason string `json:"reason"`
	Raw    string `json:"raw,omitempty"`
}

type Diagnostics struct {
	SchemaVersion              int              `json:"schema_version"`
	SourceRoot                 string           `json:"source_root"`
	Unresolved                 []Diagnostic     `json:"unresolved"`
	WriterExpansionChecks      []ExpansionCheck `json:"writer_expansion_checks"`
	WriterExpansionDifferences []ExpansionCheck `json:"writer_expansion_differences"`
}

type ExpansionCheck struct {
	Helper    string `json:"helper"`
	Requested string `json:"requested"`
	Actual    string `json:"actual"`
	Matches   bool   `json:"matches"`
	File      string `json:"file"`
	Line      int    `json:"line"`
}

type typeRef struct {
	Kind string // named, slice, array, optional, pointer, unknown
	Name string // package.Type for named
	Elem *typeRef
	Len  int
}

type fieldInfo struct {
	Name string
	Type typeRef
}
type typeInfo struct {
	Key, Name, Pkg string
	Fields         map[string]typeRef
	Embedded       []typeRef
	File           *sourceFile
}
type marshalInfo struct {
	Key, Recv, IO string
	Decl          *ast.FuncDecl
	File          *sourceFile
}
type sourceFile struct {
	Path, Rel, Pkg string
	AST            *ast.File
	Imports        map[string]string
}

type extractor struct {
	root        string
	fset        *token.FileSet
	files       []*sourceFile
	types       map[string]*typeInfo
	marshals    map[string]*marshalInfo
	functions   map[string]*marshalInfo
	diagnostics []Diagnostic
	packet      string
}

var primitive = map[string]string{
	"Uint8": "U8", "Int8": "I8", "Uint16": "U16LE", "Int16": "I16LE",
	"Uint32": "U32LE", "Int32": "I32LE", "BEInt32": "I32BE",
	"Uint64": "U64LE", "Int64": "I64LE", "Float32": "F32LE", "Float64": "F64LE",
	"Bool": "Bool", "Varint32": "ZigZag32", "Varuint32": "VarInt",
	"Varint64": "ZigZag64", "Varuint64": "VarLong",
	"ActorRuntimeID": "VarLong", "ActorRuntimeIDVarint64": "ZigZag64",
	"ActorRuntimeIDVaruint32": "VarInt", "ActorUniqueID": "ZigZag64",
	"ActorUniqueIDInt64": "I64LE", "ActorUniqueIDUint64": "U64LE",
	"ActorUniqueIDVaruint64": "VarLong",
}

var arrayPrefixes = map[string]string{
	"Slice": "VarInt", "FuncSlice": "VarInt", "FuncIOSlice": "VarInt", "SliceOfFunc": "VarInt",
	"SliceUint8Length": "U8", "SliceUint16Length": "U16LE",
	"SliceUint32Length": "U32LE", "SliceVarint32Length": "ZigZag32",
	"FuncSliceUint8Length": "U8", "FuncSliceUint16Length": "U16LE",
	"FuncSliceUint32Length": "U32LE", "FuncSliceVarint32Length": "ZigZag32",
}

func isProtocolHelper(name string) bool {
	if _, ok := arrayPrefixes[name]; ok {
		return true
	}
	switch name {
	case "SliceOfLen", "FuncSliceOfLen", "FuncIOSliceOfLen",
		"Optional", "OptionalFunc", "DoubleOptionalFunc", "OptionalMarshaler", "Single":
		return true
	}
	return false
}

func main() {
	root := filepath.Join("..", "gt-2168")
	if len(os.Args) > 1 {
		root = os.Args[1]
	}
	abs, err := filepath.Abs(root)
	must(err)
	e := &extractor{root: abs, fset: token.NewFileSet(), types: map[string]*typeInfo{}, marshals: map[string]*marshalInfo{}, functions: map[string]*marshalInfo{}}
	must(e.load())
	ids, packetTypes, err := e.packetIDs()
	must(err)
	var packets []Packet
	keys := make([]string, 0, len(packetTypes))
	for k := range packetTypes {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, key := range keys {
		idName := packetTypes[key]
		id, ok := ids[idName]
		if !ok {
			e.addDiag(nil, key, "", "packet ID constant could not be evaluated", idName)
			continue
		}
		e.packet = shortType(key)
		ops := e.expandType(key, e.packet, 0, map[string]bool{})
		if ops == nil {
			ops = []Operation{}
		}
		packets = append(packets, Packet{ID: id, Name: e.packet, Operations: ops})
	}
	sort.Slice(packets, func(i, j int) bool { return packets[i].ID < packets[j].ID })
	manifest := Manifest{SchemaVersion: 1, Source: "gophertunnel@4815aff7", MinecraftVersion: "1.26.40", ProtocolVersion: 2168, Packets: packets}
	checks := writerChecks(abs)
	diag := Diagnostics{SchemaVersion: 1, SourceRoot: abs, Unresolved: e.diagnostics, WriterExpansionChecks: checks}
	for _, c := range checks {
		if !c.Matches {
			diag.WriterExpansionDifferences = append(diag.WriterExpansionDifferences, c)
		}
	}
	must(writeJSON("gophertunnel-flat.json", manifest))
	must(writeJSON("gtx2-diagnostics.json", diag))
	fmt.Fprintf(os.Stderr, "gtx2: packets=%d marshal_types=%d unresolved=%d writer_differences=%d\n", len(packets), len(e.marshals), len(e.diagnostics), len(diag.WriterExpansionDifferences))
}

func must(err error) {
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
func writeJSON(path string, v any) error {
	b, err := json.MarshalIndent(v, "", "  ")
	if err != nil {
		return err
	}
	b = append(b, '\n')
	return os.WriteFile(path, b, 0644)
}

func (e *extractor) load() error {
	base := filepath.Join(e.root, "minecraft", "protocol")
	err := filepath.WalkDir(base, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		if !strings.HasSuffix(d.Name(), ".go") || strings.HasSuffix(d.Name(), "_test.go") {
			return nil
		}
		af, err := parser.ParseFile(e.fset, path, nil, parser.SkipObjectResolution)
		if err != nil {
			return err
		}
		rel, _ := filepath.Rel(base, path)
		sf := &sourceFile{Path: path, Rel: filepath.ToSlash(rel), Pkg: af.Name.Name, AST: af, Imports: map[string]string{}}
		for _, im := range af.Imports {
			p, _ := strconv.Unquote(im.Path.Value)
			alias := filepath.Base(p)
			if im.Name != nil {
				alias = im.Name.Name
			}
			sf.Imports[alias] = alias
		}
		e.files = append(e.files, sf)
		return nil
	})
	if err != nil {
		return err
	}
	for _, sf := range e.files { // structs first
		for _, d := range sf.AST.Decls {
			gd, ok := d.(*ast.GenDecl)
			if !ok || gd.Tok != token.TYPE {
				continue
			}
			for _, sp := range gd.Specs {
				ts := sp.(*ast.TypeSpec)
				st, ok := ts.Type.(*ast.StructType)
				if !ok {
					continue
				}
				key := sf.Pkg + "." + ts.Name.Name
				ti := &typeInfo{Key: key, Name: ts.Name.Name, Pkg: sf.Pkg, Fields: map[string]typeRef{}, File: sf}
				for _, f := range st.Fields.List {
					tr := e.parseType(f.Type, sf)
					if len(f.Names) == 0 {
						ti.Embedded = append(ti.Embedded, tr)
						ti.Fields[shortType(tr.Name)] = tr
					} else {
						for _, n := range f.Names {
							ti.Fields[n.Name] = tr
						}
					}
				}
				e.types[key] = ti
			}
		}
	}
	for _, sf := range e.files {
		for _, d := range sf.AST.Decls {
			fd, ok := d.(*ast.FuncDecl)
			if !ok || fd.Body == nil {
				continue
			}
			if fd.Recv == nil {
				e.functions[sf.Pkg+"."+fd.Name.Name] = &marshalInfo{Key: sf.Pkg + "." + fd.Name.Name, Decl: fd, File: sf}
				continue
			}
			if fd.Name.Name != "Marshal" {
				continue
			}
			rn, rv := receiver(fd)
			if rn == "" {
				continue
			}
			key := sf.Pkg + "." + rn
			ioName := ""
			if fd.Type.Params != nil && len(fd.Type.Params.List) > 0 && len(fd.Type.Params.List[0].Names) > 0 {
				ioName = fd.Type.Params.List[0].Names[0].Name
			}
			e.marshals[key] = &marshalInfo{Key: key, Recv: rv, IO: ioName, Decl: fd, File: sf}
		}
	}
	return nil
}

func receiver(fd *ast.FuncDecl) (string, string) {
	if len(fd.Recv.List) == 0 {
		return "", ""
	}
	f := fd.Recv.List[0]
	rv := ""
	if len(f.Names) > 0 {
		rv = f.Names[0].Name
	}
	switch t := f.Type.(type) {
	case *ast.Ident:
		return t.Name, rv
	case *ast.StarExpr:
		if x, ok := t.X.(*ast.Ident); ok {
			return x.Name, rv
		}
	}
	return "", rv
}

func (e *extractor) parseType(x ast.Expr, sf *sourceFile) typeRef {
	switch t := x.(type) {
	case *ast.Ident:
		return typeRef{Kind: "named", Name: sf.Pkg + "." + t.Name}
	case *ast.SelectorExpr:
		if p, ok := t.X.(*ast.Ident); ok {
			return typeRef{Kind: "named", Name: p.Name + "." + t.Sel.Name}
		}
	case *ast.StarExpr:
		z := e.parseType(t.X, sf)
		return typeRef{Kind: "pointer", Elem: &z}
	case *ast.ArrayType:
		z := e.parseType(t.Elt, sf)
		if t.Len == nil {
			return typeRef{Kind: "slice", Elem: &z}
		}
		n := literalInt(t.Len)
		return typeRef{Kind: "array", Elem: &z, Len: n}
	case *ast.IndexExpr:
		if isOptionalType(t.X) {
			z := e.parseType(t.Index, sf)
			return typeRef{Kind: "optional", Elem: &z}
		}
	case *ast.IndexListExpr:
		if isOptionalType(t.X) && len(t.Indices) == 1 {
			z := e.parseType(t.Indices[0], sf)
			return typeRef{Kind: "optional", Elem: &z}
		}
	}
	return typeRef{Kind: "unknown"}
}

func isOptionalType(x ast.Expr) bool {
	switch v := x.(type) {
	case *ast.Ident:
		return v.Name == "Optional"
	case *ast.SelectorExpr:
		return v.Sel.Name == "Optional"
	}
	return false
}

func literalInt(x ast.Expr) int {
	if b, ok := x.(*ast.BasicLit); ok {
		n, _ := strconv.Atoi(b.Value)
		return n
	}
	return 0
}

func (e *extractor) packetIDs() (map[string]int, map[string]string, error) {
	ids := map[string]int{}
	packetTypes := map[string]string{}
	for _, sf := range e.files {
		if sf.Pkg != "packet" {
			continue
		}
		for _, d := range sf.AST.Decls {
			if gd, ok := d.(*ast.GenDecl); ok && gd.Tok == token.CONST {
				iotaVal := 0
				var prev ast.Expr
				for _, sp := range gd.Specs {
					vs := sp.(*ast.ValueSpec)
					expr := prev
					if len(vs.Values) > 0 {
						expr = vs.Values[0]
						prev = expr
					}
					if n, ok := evalConst(expr, iotaVal, ids); ok {
						for _, nm := range vs.Names {
							if nm.Name != "_" {
								ids[nm.Name] = n
							}
						}
					}
					iotaVal++
				}
			}
			fd, ok := d.(*ast.FuncDecl)
			if !ok || fd.Name.Name != "ID" || fd.Recv == nil || fd.Body == nil {
				continue
			}
			rn, _ := receiver(fd)
			var idName string
			ast.Inspect(fd.Body, func(n ast.Node) bool {
				if id, ok := n.(*ast.Ident); ok && strings.HasPrefix(id.Name, "ID") {
					idName = id.Name
				}
				return true
			})
			if idName != "" {
				packetTypes["packet."+rn] = idName
			}
		}
	}
	return ids, packetTypes, nil
}

func evalConst(x ast.Expr, iotaVal int, ids map[string]int) (int, bool) {
	switch v := x.(type) {
	case *ast.BasicLit:
		n, e := strconv.ParseInt(v.Value, 0, 64)
		return int(n), e == nil
	case *ast.Ident:
		if v.Name == "iota" {
			return iotaVal, true
		}
		n, ok := ids[v.Name]
		return n, ok
	case *ast.ParenExpr:
		return evalConst(v.X, iotaVal, ids)
	case *ast.BinaryExpr:
		a, ao := evalConst(v.X, iotaVal, ids)
		b, bo := evalConst(v.Y, iotaVal, ids)
		if !ao || !bo {
			return 0, false
		}
		switch v.Op {
		case token.ADD:
			return a + b, true
		case token.SUB:
			return a - b, true
		case token.SHL:
			return a << b, true
		case token.OR:
			return a | b, true
		}
	}
	return 0, false
}

func (e *extractor) expandType(key, field string, depth int, stack map[string]bool) []Operation {
	if depth > depthLimit {
		return []Operation{e.unresolved(nil, key, field, "recursion depth limit exceeded", key)}
	}
	if stack[key] {
		return []Operation{{Kind: "recursive_reference", Field: field, TypeName: key}}
	}
	mi := e.marshals[key]
	if mi == nil {
		return []Operation{e.unresolved(nil, key, field, "target type has no Marshal method", key)}
	}
	ns := cloneSet(stack)
	ns[key] = true
	env := map[string]typeRef{mi.Recv: {Kind: "named", Name: key}}
	return e.extractBlock(mi.Decl.Body.List, mi, env, field, depth, ns)
}

func cloneSet(x map[string]bool) map[string]bool {
	r := map[string]bool{}
	for k, v := range x {
		r[k] = v
	}
	return r
}

func (e *extractor) extractBlock(stmts []ast.Stmt, mi *marshalInfo, env map[string]typeRef, base string, depth int, stack map[string]bool) []Operation {
	var out []Operation
	for _, st := range stmts {
		switch s := st.(type) {
		case *ast.ExprStmt:
			if c, ok := s.X.(*ast.CallExpr); ok {
				out = append(out, e.extractCall(c, mi, env, base, depth, stack)...)
			}
		case *ast.IfStmt:
			out = append(out, e.unresolved(s, mi.Key, base, "conditional branch cannot be represented as a single wire sequence", e.nodeString(s.Cond)))
		case *ast.SwitchStmt:
			out = append(out, e.unresolved(s, mi.Key, base, "conditional switch/union requires runtime discriminator", e.nodeString(s.Tag)))
		case *ast.TypeSwitchStmt:
			out = append(out, e.unresolved(s, mi.Key, base, "type switch/union requires runtime type", e.nodeString(s.Assign)))
		case *ast.ForStmt, *ast.RangeStmt:
			out = append(out, e.unresolved(st, mi.Key, base, "loop outside a recognised slice helper", e.nodeString(st)))
		case *ast.DeferStmt:
			out = append(out, e.unresolved(s, mi.Key, base, "deferred call in Marshal", e.nodeString(s.Call)))
		}
	}
	return out
}

func (e *extractor) extractCall(c *ast.CallExpr, mi *marshalInfo, env map[string]typeRef, base string, depth int, stack map[string]bool) []Operation {
	se, ok := c.Fun.(*ast.SelectorExpr)
	if !ok {
		if id, yes := c.Fun.(*ast.Ident); yes && mi.File.Pkg == "protocol" {
			if isProtocolHelper(id.Name) {
				return e.expandProtocol(c, id.Name, mi, env, base, depth, stack)
			}
		}
		return []Operation{e.unresolved(c, mi.Key, base, "unrecognised function call in Marshal", e.nodeString(c))}
	}
	method := se.Sel.Name
	if id, ok := se.X.(*ast.Ident); ok && id.Name == mi.IO {
		if method == "UnknownEnumOption" || method == "InvalidValue" {
			return nil
		}
		field := base
		if len(c.Args) > 0 {
			field = e.fieldPath(c.Args[0], mi, env, base)
		}
		if p, ok := primitive[method]; ok {
			return []Operation{{Kind: "primitive", Field: field, Op: p}}
		}
		return e.expandIOHelper(c, method, field, mi)
	}
	if id, ok := se.X.(*ast.Ident); ok && id.Name == "protocol" {
		return e.expandProtocol(c, method, mi, env, base, depth, stack)
	}
	if method == "Marshal" {
		tr := e.resolveExprType(se.X, mi, env)
		if tr.Kind == "pointer" && tr.Elem != nil {
			tr = *tr.Elem
		}
		field := e.fieldPath(se.X, mi, env, base)
		if tr.Kind != "named" {
			return []Operation{e.unresolved(c, mi.Key, field, "Marshal receiver type could not be resolved", e.nodeString(se.X))}
		}
		return e.expandType(tr.Name, field, depth+1, stack)
	}
	// Calls to conversion/validation helpers are non-wire only when their return value is ignored and
	// their names explicitly say so. Every other statement call is conservatively opaque.
	if strings.HasSuffix(method, "Value") {
		return nil
	}
	return []Operation{e.unresolved(c, mi.Key, base, "opaque method/helper call in Marshal", e.nodeString(c))}
}

func (e *extractor) expandIOHelper(c *ast.CallExpr, method, field string, mi *marshalInfo) []Operation {
	primN := func(op string, n int) []Operation {
		r := make([]Operation, n)
		axis := []string{"X", "Y", "Z"}
		for i := range r {
			f := field
			if n > 1 {
				f += "." + axis[i]
			}
			r[i] = Operation{Kind: "primitive", Field: f, Op: op}
		}
		return r
	}
	switch method {
	case "Vec3":
		return primN("F32LE", 3)
	case "Vec2":
		return primN("F32LE", 2)
	case "BlockPos":
		return primN("ZigZag32", 3)
	case "SubChunkPos":
		return primN("I32LE", 3)
	case "ChunkPos":
		return primN("ZigZag32", 2)
	case "SoundPos":
		return primN("ZigZag32", 3)
	case "ByteFloat":
		return primN("U8", 1)
	case "UUID":
		return []Operation{{Kind: "fixed_array", Field: field, Length: 16, Element: []Operation{{Kind: "primitive", Field: field + "[]", Op: "U8"}}}}
	case "String":
		return []Operation{{Kind: "string", Field: field, Prefix: "VarInt", Encoding: "utf8"}}
	case "StringUTF":
		return []Operation{{Kind: "string", Field: field, Prefix: "I16LE", Encoding: "utf8"}}
	case "ByteSlice":
		return []Operation{{Kind: "string", Field: field, Prefix: "VarInt", Encoding: "bytes"}}
	case "NBT":
		return []Operation{{Kind: "primitive", Field: field, Op: "Nbt"}}
	case "Bytes":
		return []Operation{{Kind: "primitive", Field: field, Op: "RawBytes"}}
	default:
		return []Operation{e.unresolved(c, mi.Key, field, "opaque or unsupported IO helper", method)}
	}
}

func (e *extractor) expandProtocol(c *ast.CallExpr, method string, mi *marshalInfo, env map[string]typeRef, base string, depth int, stack map[string]bool) []Operation {
	if prefix, ok := arrayPrefixes[method]; ok {
		if len(c.Args) < 2 {
			return []Operation{e.unresolved(c, mi.Key, base, "slice helper has too few arguments", method)}
		}
		field := e.fieldPath(c.Args[1], mi, env, base)
		tr := e.resolveExprType(c.Args[1], mi, env)
		for tr.Kind == "pointer" && tr.Elem != nil {
			tr = *tr.Elem
		}
		if tr.Kind != "slice" && tr.Kind != "array" {
			return []Operation{e.unresolved(c, mi.Key, field, "slice field type could not be resolved", e.nodeString(c.Args[1]))}
		}
		elem := *tr.Elem
		var child []Operation
		if strings.HasPrefix(method, "Func") || method == "SliceOfFunc" {
			if len(c.Args) < 3 {
				child = []Operation{e.unresolved(c, mi.Key, field+"[]", "FuncSlice callback missing", method)}
			} else {
				child = e.callbackOps(c.Args[2], elem, mi, env, field+"[]", depth, stack)
			}
		} else {
			child = e.expandRef(elem, field+"[]", depth+1, stack, c, mi)
		}
		return []Operation{{Kind: "array", Field: field, Prefix: prefix, Element: child}}
	}
	if method == "SliceOfLen" || method == "FuncSliceOfLen" || method == "FuncIOSliceOfLen" {
		if len(c.Args) < 3 {
			return []Operation{e.unresolved(c, mi.Key, base, "slice-of-length helper has too few arguments", method)}
		}
		field := e.fieldPath(c.Args[2], mi, env, base)
		tr := e.resolveExprType(c.Args[2], mi, env)
		for tr.Kind == "pointer" && tr.Elem != nil {
			tr = *tr.Elem
		}
		if (tr.Kind != "slice" && tr.Kind != "array") || tr.Elem == nil {
			return []Operation{e.unresolved(c, mi.Key, field, "slice field type could not be resolved", e.nodeString(c.Args[2]))}
		}
		length := literalInt(c.Args[1])
		if length <= 0 {
			return []Operation{e.unresolved(c, mi.Key, field, "slice helper uses an external runtime length that cannot be represented", e.nodeString(c.Args[1]))}
		}
		var child []Operation
		if method == "SliceOfLen" {
			child = e.expandRef(*tr.Elem, field+"[]", depth+1, stack, c, mi)
		} else if len(c.Args) < 4 {
			child = []Operation{e.unresolved(c, mi.Key, field+"[]", "slice callback missing", method)}
		} else {
			child = e.callbackOps(c.Args[3], *tr.Elem, mi, env, field+"[]", depth, stack)
		}
		return []Operation{{Kind: "fixed_array", Field: field, Length: length, Element: child}}
	}
	if method == "Optional" || method == "OptionalFunc" || method == "DoubleOptionalFunc" || method == "OptionalMarshaler" {
		if len(c.Args) < 2 {
			return []Operation{e.unresolved(c, mi.Key, base, "optional helper has too few arguments", method)}
		}
		field := e.fieldPath(c.Args[1], mi, env, base)
		tr := e.resolveExprType(c.Args[1], mi, env)
		for tr.Kind == "pointer" && tr.Elem != nil {
			tr = *tr.Elem
		}
		if tr.Kind != "optional" || tr.Elem == nil {
			return []Operation{e.unresolved(c, mi.Key, field, "optional field type could not be resolved", e.nodeString(c.Args[1]))}
		}
		var child []Operation
		if (method == "OptionalFunc" || method == "DoubleOptionalFunc") && len(c.Args) >= 3 {
			child = e.callbackOps(c.Args[2], *tr.Elem, mi, env, field, depth, stack)
		} else {
			child = e.expandRef(*tr.Elem, field, depth+1, stack, c, mi)
		}
		inner := Operation{Kind: "option", Field: field, Presence: "Bool", Value: child}
		if method == "DoubleOptionalFunc" {
			return []Operation{{Kind: "option", Field: field, Presence: "Bool", Value: []Operation{inner}}}
		}
		return []Operation{inner}
	}
	if method == "Single" {
		if len(c.Args) < 2 {
			return []Operation{e.unresolved(c, mi.Key, base, "Single helper has too few arguments", method)}
		}
		field := e.fieldPath(c.Args[1], mi, env, base)
		tr := e.resolveExprType(c.Args[1], mi, env)
		for tr.Kind == "pointer" && tr.Elem != nil {
			tr = *tr.Elem
		}
		return e.expandRef(tr, field, depth+1, stack, c, mi)
	}
	return []Operation{e.unresolved(c, mi.Key, base, "unsupported protocol helper", method)}
}

func (e *extractor) callbackOps(fn ast.Expr, elem typeRef, mi *marshalInfo, env map[string]typeRef, field string, depth int, stack map[string]bool) []Operation {
	if se, ok := fn.(*ast.SelectorExpr); ok {
		if id, yes := se.X.(*ast.Ident); yes && id.Name == mi.IO {
			fake := &ast.CallExpr{Fun: se, Args: []ast.Expr{ast.NewIdent(field)}}
			if p, yes := primitive[se.Sel.Name]; yes {
				return []Operation{{Kind: "primitive", Field: field, Op: p}}
			}
			return e.expandIOHelper(fake, se.Sel.Name, field, mi)
		}
		if se.Sel.Name == "Marshal" {
			tr := e.resolveExprType(se.X, mi, env)
			for tr.Kind == "pointer" && tr.Elem != nil {
				tr = *tr.Elem
			}
			if tr.Kind == "named" {
				if target := e.marshals[tr.Name]; target != nil {
					return e.callbackDecl(target, elem, field, depth, stack)
				}
			}
		}
		if id, yes := se.X.(*ast.Ident); yes && id.Name == "protocol" {
			if target := e.functions["protocol."+se.Sel.Name]; target != nil {
				return e.callbackDecl(target, elem, field, depth, stack)
			}
		}
	}
	if fl, ok := fn.(*ast.FuncLit); ok {
		nenv := map[string]typeRef{}
		for k, v := range env {
			nenv[k] = v
		}
		cmi := *mi
		if fl.Type.Params != nil {
			for pi, p := range fl.Type.Params.List {
				for _, n := range p.Names {
					pt := e.parseType(p.Type, mi.File)
					if shortType(pt.Name) == "IO" {
						cmi.IO = n.Name
						nenv[n.Name] = pt
					} else if pi == len(fl.Type.Params.List)-1 {
						nenv[n.Name] = elem
					} else {
						nenv[n.Name] = pt
					}
				}
			}
		}
		return e.extractBlock(fl.Body.List, &cmi, nenv, field, depth+1, stack)
	}
	if id, ok := fn.(*ast.Ident); ok && mi.File.Pkg == "protocol" {
		if target := e.functions["protocol."+id.Name]; target != nil {
			return e.callbackDecl(target, elem, field, depth, stack)
		}
	}
	return []Operation{e.unresolved(fn, mi.Key, field, "slice/optional callback could not be resolved", e.nodeString(fn))}
}

func (e *extractor) callbackDecl(target *marshalInfo, elem typeRef, field string, depth int, stack map[string]bool) []Operation {
	ctx := *target
	env := map[string]typeRef{}
	if target.Recv != "" {
		env[target.Recv] = typeRef{Kind: "named", Name: target.Key}
	}
	params := target.Decl.Type.Params
	if params != nil {
		for pi, p := range params.List {
			for _, n := range p.Names {
				pt := e.parseType(p.Type, target.File)
				if shortType(pt.Name) == "IO" {
					ctx.IO = n.Name
					env[n.Name] = pt
				} else if pi == len(params.List)-1 {
					env[n.Name] = elem
				} else {
					env[n.Name] = pt
				}
			}
		}
	}
	if ctx.IO == "" {
		return []Operation{e.unresolved(target.Decl, target.Key, field, "named callback IO parameter could not be resolved", target.Key)}
	}
	return e.extractBlock(target.Decl.Body.List, &ctx, env, field, depth+1, stack)
}

func (e *extractor) expandRef(tr typeRef, field string, depth int, stack map[string]bool, node ast.Node, mi *marshalInfo) []Operation {
	for tr.Kind == "pointer" && tr.Elem != nil {
		tr = *tr.Elem
	}
	switch tr.Kind {
	case "named":
		return e.expandType(tr.Name, field, depth, stack)
	case "array":
		child := e.expandRef(*tr.Elem, field+"[]", depth+1, stack, node, mi)
		return []Operation{{Kind: "fixed_array", Field: field, Length: tr.Len, Element: child}}
	case "slice":
		return []Operation{e.unresolved(node, mi.Key, field, "bare slice has no known length prefix", tr.String())}
	default:
		return []Operation{e.unresolved(node, mi.Key, field, "element/value type could not be resolved", tr.String())}
	}
}

func (t typeRef) String() string {
	if t.Kind == "named" {
		return t.Name
	}
	if t.Elem != nil {
		return t.Kind + "[" + t.Elem.String() + "]"
	}
	return t.Kind
}

func (e *extractor) resolveExprType(x ast.Expr, mi *marshalInfo, env map[string]typeRef) typeRef {
	switch v := x.(type) {
	case *ast.UnaryExpr:
		return e.resolveExprType(v.X, mi, env)
	case *ast.ParenExpr:
		return e.resolveExprType(v.X, mi, env)
	case *ast.StarExpr:
		return e.resolveExprType(v.X, mi, env)
	case *ast.Ident:
		if t, ok := env[v.Name]; ok {
			return t
		}
		return typeRef{Kind: "unknown"}
	case *ast.IndexExpr:
		t := e.resolveExprType(v.X, mi, env)
		for t.Kind == "pointer" && t.Elem != nil {
			t = *t.Elem
		}
		if (t.Kind == "slice" || t.Kind == "array") && t.Elem != nil {
			return *t.Elem
		}
		return typeRef{Kind: "unknown"}
	case *ast.SelectorExpr:
		t := e.resolveExprType(v.X, mi, env)
		for t.Kind == "pointer" && t.Elem != nil {
			t = *t.Elem
		}
		if t.Kind != "named" {
			return typeRef{Kind: "unknown"}
		}
		return e.lookupField(t.Name, v.Sel.Name, map[string]bool{})
	case *ast.CompositeLit:
		return e.parseType(v.Type, mi.File)
	}
	return typeRef{Kind: "unknown"}
}

func (e *extractor) lookupField(key, name string, seen map[string]bool) typeRef {
	if seen[key] {
		return typeRef{Kind: "unknown"}
	}
	seen[key] = true
	ti := e.types[key]
	if ti == nil {
		return typeRef{Kind: "unknown"}
	}
	if t, ok := ti.Fields[name]; ok {
		return t
	}
	for _, em := range ti.Embedded {
		t := em
		for t.Kind == "pointer" && t.Elem != nil {
			t = *t.Elem
		}
		if t.Kind == "named" {
			z := e.lookupField(t.Name, name, seen)
			if z.Kind != "unknown" {
				return z
			}
		}
	}
	return typeRef{Kind: "unknown"}
}

func (e *extractor) fieldPath(x ast.Expr, mi *marshalInfo, env map[string]typeRef, fallback string) string {
	var parts []string
	var walk func(ast.Expr) bool
	walk = func(y ast.Expr) bool {
		switch v := y.(type) {
		case *ast.UnaryExpr:
			return walk(v.X)
		case *ast.ParenExpr:
			return walk(v.X)
		case *ast.IndexExpr:
			ok := walk(v.X)
			if ok {
				parts = append(parts, "[]")
			}
			return ok
		case *ast.Ident:
			if v.Name == mi.Recv {
				return true
			}
			if _, ok := env[v.Name]; ok {
				return true
			}
			return false
		case *ast.SelectorExpr:
			if walk(v.X) {
				parts = append(parts, v.Sel.Name)
				return true
			}
		}
		return false
	}
	if !walk(x) || len(parts) == 0 {
		return fallback
	}
	p := fallback
	if p == "" {
		p = parts[0]
		parts = parts[1:]
	}
	for _, s := range parts {
		if s == "[]" {
			p += s
		} else if p == "" {
			p = s
		} else {
			p += "." + s
		}
	}
	return p
}

func (e *extractor) unresolved(n ast.Node, typ, field, reason, raw string) Operation {
	site := ""
	if n != nil {
		p := e.fset.Position(n.Pos())
		site = filepath.ToSlash(p.Filename) + ":" + strconv.Itoa(p.Line)
		e.diagnostics = append(e.diagnostics, Diagnostic{Packet: e.packet, Type: typ, Field: field, File: filepath.ToSlash(p.Filename), Line: p.Line, Reason: reason, Raw: raw})
	} else {
		e.diagnostics = append(e.diagnostics, Diagnostic{Packet: e.packet, Type: typ, Field: field, File: "", Line: 0, Reason: reason, Raw: raw})
	}
	return Operation{Kind: "unresolved", Field: field, Reason: reason, Site: site}
}
func (e *extractor) addDiag(n ast.Node, typ, field, reason, raw string) {
	e.unresolved(n, typ, field, reason, raw)
}
func (e *extractor) nodeString(n ast.Node) string {
	if n == nil {
		return ""
	}
	var b strings.Builder
	_ = printer.Fprint(&b, e.fset, n)
	s := b.String()
	if len(s) > 300 {
		s = s[:300] + "..."
	}
	return s
}
func shortType(k string) string {
	if i := strings.LastIndex(k, "."); i >= 0 {
		return k[i+1:]
	}
	return k
}

// These checks are intentionally explicit and tied to writer.go. The extractor uses the Actual column.
func writerChecks(root string) []ExpansionCheck {
	file := filepath.ToSlash(filepath.Join(root, "minecraft", "protocol", "writer.go"))
	req := map[string]string{"Vec3": "3x F32LE", "Vec2": "2x F32LE", "BlockPos": "3x ZigZag32", "SubChunkPos": "3x I32LE", "ChunkPos": "2x ZigZag32", "SoundPos": "3x F32LE", "ByteFloat": "U8", "String": "string(prefix=VarInt,encoding=utf8)", "StringUTF": "string(prefix=I16LE,encoding=utf8)", "ByteSlice": "string(prefix=VarInt,encoding=bytes)", "NBT": "Nbt", "Bytes": "RawBytes", "UUID": "fixed_array(16x U8)", "ActorRuntimeID": "VarLong", "ActorRuntimeIDVarint64": "ZigZag64", "ActorRuntimeIDVaruint32": "VarInt", "ActorUniqueID": "ZigZag64", "ActorUniqueIDInt64": "I64LE", "ActorUniqueIDUint64": "U64LE", "ActorUniqueIDVaruint64": "VarLong"}
	actual := map[string]string{"Vec3": "3x F32LE", "Vec2": "2x F32LE", "BlockPos": "3x ZigZag32", "SubChunkPos": "3x I32LE", "ChunkPos": "2x ZigZag32", "SoundPos": "3x ZigZag32", "ByteFloat": "U8", "String": "string(prefix=VarInt,encoding=utf8)", "StringUTF": "string(prefix=I16LE,encoding=utf8)", "ByteSlice": "string(prefix=VarInt,encoding=bytes)", "NBT": "Nbt", "Bytes": "RawBytes", "UUID": "fixed_array(16x U8)", "ActorRuntimeID": "VarLong", "ActorRuntimeIDVarint64": "ZigZag64", "ActorRuntimeIDVaruint32": "VarInt", "ActorUniqueID": "ZigZag64", "ActorUniqueIDInt64": "I64LE", "ActorUniqueIDUint64": "U64LE", "ActorUniqueIDVaruint64": "VarLong"}
	lines := map[string]int{"StringUTF": 67, "String": 74, "ByteSlice": 95, "Bytes": 102, "ByteFloat": 107, "Vec3": 112, "Vec2": 119, "BlockPos": 125, "ChunkPos": 132, "SubChunkPos": 137, "SoundPos": 144, "UUID": 172, "ActorRuntimeID": 527, "ActorRuntimeIDVarint64": 532, "ActorRuntimeIDVaruint32": 537, "ActorUniqueID": 542, "ActorUniqueIDInt64": 547, "ActorUniqueIDUint64": 552, "ActorUniqueIDVaruint64": 557, "NBT": 596}
	names := make([]string, 0, len(req))
	for n := range req {
		names = append(names, n)
	}
	sort.Strings(names)
	out := make([]ExpansionCheck, 0, len(names))
	for _, n := range names {
		out = append(out, ExpansionCheck{Helper: n, Requested: req[n], Actual: actual[n], Matches: req[n] == actual[n], File: file, Line: lines[n]})
	}
	return out
}
