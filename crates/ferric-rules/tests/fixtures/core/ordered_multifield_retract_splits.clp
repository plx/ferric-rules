;; Retracting one fact removes every partition and descendant activation.
(defglobal ?*direct* = 0 ?*joined* = 0 ?*retracted* = 0)
(deffacts input (row a marker b marker c) (witness yes) (trigger))
(defrule direct
  (row $?left marker $?right)
  => (bind ?*direct* (+ ?*direct* 1)))
(defrule descendant
  (row $?left marker $?right)
  (witness yes)
  => (bind ?*joined* (+ ?*joined* 1)))
(defrule remove
  (declare (salience 10))
  ?f <- (row $?left marker $?right)
  ?trigger <- (trigger)
  =>
  (retract ?f ?trigger)
  (bind ?*retracted* (+ ?*retracted* 1)))
(defrule summary
  (declare (salience -10))
  => (printout t ?*direct* ":" ?*joined* ":" ?*retracted* crlf))
