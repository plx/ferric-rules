; Pinned CLIPS 6.30 transition trace for a leading simple NOT CE.
; The rule set observes the empty state, assertion blocking, and reactivation
; after the last blocker is retracted.
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
    (declare (salience 0))
    (not (blocker))
    ?phase <- (phase final)
    =>
    (printout t "reactivated" crlf)
    (retract ?phase))
