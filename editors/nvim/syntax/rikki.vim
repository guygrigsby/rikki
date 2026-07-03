" rikki syntax highlighting: lexical classes only, mirrors src/lexer.rs.
if exists("b:current_syntax")
  finish
endif

syn keyword rikkiKeyword fn struct import return if else for range break continue check
syn keyword rikkiType int float bool str error py map
syn keyword rikkiBoolean true false
syn keyword rikkiConstant none
syn keyword rikkiBuiltin print printf sprintf len append ord chr args input

syn match rikkiComment "//.*$" contains=@Spell
" the lexer's escapes are exactly \n \t \" \\; strings are one line
syn match rikkiEscape contained +\\[nt"\\]+
syn region rikkiString start=+"+ skip=+\\"+ end=+"+ oneline contains=rikkiEscape
syn match rikkiNumber "\<\d\+\%(\.\d\+\)\?\>"
syn match rikkiOperator ":=\|==\|!=\|<=\|>=\|&&\|||\|[-+*/%@!<>?=]"
" name in a fn declaration
syn match rikkiFuncDef "\%(\<fn\s\+\)\@<=\w\+"

hi def link rikkiKeyword Keyword
hi def link rikkiType Type
hi def link rikkiBoolean Boolean
hi def link rikkiConstant Constant
hi def link rikkiBuiltin Function
hi def link rikkiComment Comment
hi def link rikkiString String
hi def link rikkiEscape SpecialChar
hi def link rikkiNumber Number
hi def link rikkiOperator Operator
hi def link rikkiFuncDef Function

let b:current_syntax = "rikki"
