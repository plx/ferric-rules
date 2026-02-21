;; Phase 4: String and symbol function surface fixture.
;; Exercises str-cat, sym-cat, str-length, sub-string, format.

(defrule test-strings
    (go)
    =>
    (printout t "str-cat: " (str-cat "hello" " " "world") crlf)
    (printout t "sym-cat: " (sym-cat foo bar) crlf)
    (printout t "str-length: " (str-length "hello") crlf)
    (printout t "sub-string: " (sub-string 1 5 "hello world") crlf)
    (printout t "format: " (format nil "val=%d pi=%.2f" 42 3.14159) crlf))

(deffacts startup (go))
