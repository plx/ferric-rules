; User-defined function using str-cat, called from rule RHS.
; deffunction body is a pure expression; printout happens in the rule RHS.
(deffunction greet (?name) (str-cat "Hello " ?name "!"))
(deffacts startup (person Alice))

(defrule say-hi
    (person ?name)
    =>
    (printout t (greet ?name) crlf))
