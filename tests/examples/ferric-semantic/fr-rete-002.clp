; FR-RETE-002: a leading negative CE must seed, block, and reactivate.
(deffacts startup
  (phase initial))

(defrule observe-empty
  (declare (salience 40))
  (not (blocker))
  ?phase <- (phase initial)
  =>
  (printout t "empty" crlf)
  (retract ?phase)
  (assert (phase add-blocker)))

(defrule add-blocker
  (declare (salience 30))
  ?phase <- (phase add-blocker)
  =>
  (printout t "assert-blocker" crlf)
  (retract ?phase)
  (assert (blocker))
  (assert (phase observe-blocked)))

(defrule observe-blocked
  (declare (salience 20))
  (blocker)
  ?phase <- (phase observe-blocked)
  =>
  (printout t "blocked" crlf)
  (retract ?phase)
  (assert (phase retract-blocker)))

(defrule retract-blocker
  (declare (salience 10))
  ?phase <- (phase retract-blocker)
  ?blocker <- (blocker)
  =>
  (printout t "retract-blocker" crlf)
  (retract ?phase ?blocker)
  (assert (phase final)))

(defrule observe-reactivated
  (not (blocker))
  ?phase <- (phase final)
  =>
  (printout t "reactivated" crlf)
  (retract ?phase)
  (assert (result reactivated)))
