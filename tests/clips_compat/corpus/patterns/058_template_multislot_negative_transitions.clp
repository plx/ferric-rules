;; Removing one of two NCC matches leaves its owner blocked.
(deftemplate item (slot key) (multislot tags))
(defglobal ?*not* = 0 ?*exists* = 0 ?*ncc* = 0)
(deffacts input (anchor yes) (phase start))
(defrule absent
  (anchor yes) (phase ?phase)
  (not (item (key retained) (tags $?left marker $?right)))
  => (bind ?*not* (+ ?*not* 1)))
(defrule present
  (anchor yes) (phase ?phase)
  (exists (item (key retained) (tags $?left marker $?right)))
  => (bind ?*exists* (+ ?*exists* 1)))
(defrule no-supported-prefix
  (anchor yes) (phase ?phase)
  (not (and (item (key retained) (tags $?left marker $?right)) (permit $?left)))
  => (bind ?*ncc* (+ ?*ncc* 1)))
(defrule start
  (declare (salience -10))
  ?phase <- (phase start)
  => (retract ?phase)
  (assert (item (key retained) (tags a marker b marker c)) (permit a) (permit a marker b) (phase blocked)))
(defrule remove-one-blocker
  (declare (salience -10))
  ?phase <- (phase blocked)
  ?permit <- (permit a)
  => (retract ?phase ?permit) (assert (phase one-blocker)))
(defrule remove-last-blocker
  (declare (salience -10))
  ?phase <- (phase one-blocker)
  ?permit <- (permit a marker b)
  => (retract ?phase ?permit) (assert (phase unblocked)))
(defrule remove-row
  (declare (salience -10))
  ?phase <- (phase unblocked)
  ?row <- (item (tags $?))
  => (retract ?phase ?row) (assert (phase removed)))
(defrule summary
  (declare (salience -20))
  (phase removed)
  => (printout t ?*not* ":" ?*exists* ":" ?*ncc* crlf))
