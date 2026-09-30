;; Sorting mixed strings and symbols keeps each field's type.
;; Level: boundary
;; Covers: >, create$, deffunction, progn$, sort, str-compare, stringp, symbolp
;; Run with load, reset, and run in a fresh environment.

(deffunction exchange (?a ?b) (> (str-compare ?a ?b) 0))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (progn$ (?x (sort exchange (create$ "z" a "b")))
        (printout t (stringp ?x) ":" (symbolp ?x) ":" ?x crlf)))
