; String operations: sym-cat, str-length, sub-string.
(deffacts startup (run-strings))

(defrule strings
    (run-strings)
    =>
    (printout t "sym-cat: " (sym-cat abc def) crlf)
    (printout t "str-len: " (str-length "hello world") crlf)
    (printout t "sub-str: " (sub-string 1 5 "hello world") crlf))
