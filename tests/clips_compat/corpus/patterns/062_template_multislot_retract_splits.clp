;; Retraction removes every split and downstream activation of the fact.
(deftemplate item (slot key) (multislot tags))
(defglobal ?*direct* = 0 ?*joined* = 0 ?*retracted* = 0)
(deffacts input (item (key retained) (tags a marker b marker c)) (witness yes) (trigger))
(defrule direct
  (item (tags $?left marker $?right) (key retained))
  => (bind ?*direct* (+ ?*direct* 1)))
(defrule descendant
  (item (key ?key) (tags $?left marker $?right))
  (witness yes)
  => (bind ?*joined* (+ ?*joined* 1)))
(defrule remove (declare (salience 10))
  ?f <- (item (tags $?left marker $?right) (key retained))
  ?trigger <- (trigger)
  => (retract ?f ?trigger) (bind ?*retracted* (+ ?*retracted* 1)))
(defrule summary (declare (salience -10))
  => (printout t ?*direct* ":" ?*joined* ":" ?*retracted* crlf))
