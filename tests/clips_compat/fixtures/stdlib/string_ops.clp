; String operations: str-cat, str-length, sub-string.
; sub-string is 1-indexed in CLIPS.
(deffacts startup (test-strings))

(defrule string-test
    (test-strings)
    =>
    (printout t "cat: " (str-cat "hello" " " "world") crlf)
    (printout t "len: " (str-length "hello") crlf)
    (printout t "sub: " (sub-string 1 3 "hello") crlf))
