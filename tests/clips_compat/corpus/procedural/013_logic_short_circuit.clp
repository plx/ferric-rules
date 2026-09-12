;; Logical operators skip later expressions when the result is already determined.
;; Level: boundary
;; Covers: +, and, bind, deffunction, defglobal, or, touch
;; Run with load, reset, and run in a fresh environment.

(defglobal ?*calls* = 0)
(deffunction touch () (bind ?*calls* (+ ?*calls* 1)) TRUE)

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (and FALSE (touch)) " " (or TRUE (touch)) " " ?*calls* crlf))
