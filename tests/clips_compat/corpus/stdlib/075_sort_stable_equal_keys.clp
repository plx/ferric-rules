;; Sort keeps equal keys in their original order and keeps each field's type.
;; Level: boundary
;; Covers: <, >, create$, deffunction, div, floatp, integerp, progn$, sort
;; Run with load, reset, and run in a fresh environment.

(deffunction ascending (?a ?b) (> (div ?a 10) (div ?b 10)))

(deffunction descending (?a ?b) (< (div ?a 10) (div ?b 10)))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sort ascending (create$ 21 11 22 12 23)) crlf)
    (printout t (sort descending (create$ 21 11 22 12 23)) crlf)
    (progn$ (?x (sort > (create$ 2 1.0 1 2.0 9007199254740993 9007199254740992)))
        (printout t (integerp ?x) ":" (floatp ?x) ":" ?x crlf)))
