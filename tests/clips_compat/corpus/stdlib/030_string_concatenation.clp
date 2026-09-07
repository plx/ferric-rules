;; String concatenation accepts symbols and numbers and returns STRING.
;; Level: basic
;; Covers: str-cat, stringp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (str-cat "a" b 3) " " (stringp (str-cat a b)) crlf))
