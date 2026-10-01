;; Sort invokes a defgeneric predicate once per comparison.
;; Level: interaction
;; Covers: +, <, bind, create$, defgeneric, defglobal, defmethod, sort
;; Run with load, reset, and run in a fresh environment.

(defglobal ?*calls* = 0)

(defgeneric exchange)

(defmethod exchange ((?a INTEGER) (?b INTEGER))
    (bind ?*calls* (+ ?*calls* 1))
    (< ?a ?b))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sort exchange (create$ 3 1 2)) " " ?*calls* crlf))
