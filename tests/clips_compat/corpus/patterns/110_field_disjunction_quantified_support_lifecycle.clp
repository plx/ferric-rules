;; Predicate supports cancel pending activations, survive replacement, and reactivate after retraction.
;; Level: interaction
;; Covers: patterns, field-disjunction, not, exists, join, assert, retract, salience
(deffacts seed (key a) (sym c) (phase start))
(defrule absent (key ?k) (not (sym ?k|b))
  => (printout t "absent " ?k crlf))
(defrule present (key ?k) (exists (sym ?k|b))
  => (printout t "present " ?k crlf))
;; Cancel the initial absence activation before it fires.
(defrule add-support
  (declare (salience 10))
  ?phase <- (phase start)
  => (retract ?phase) (assert (sym b) (phase added))
  (printout t "added" crlf))
;; The existential condition remains supported throughout replacement.
(defrule replace-support
  (declare (salience -10))
  ?phase <- (phase added)
  ?support <- (sym b)
  => (retract ?phase) (assert (sym a)) (retract ?support)
  (assert (phase replaced)) (printout t "replaced" crlf))
(defrule remove-support
  (declare (salience -10))
  ?phase <- (phase replaced)
  ?support <- (sym a)
  => (retract ?phase ?support) (assert (phase removed))
  (printout t "removed" crlf))
(defrule add-transient-support
  (declare (salience -10))
  ?phase <- (phase removed)
  => (retract ?phase) (assert (sym b) (phase transient))
  (printout t "transient-added" crlf))
;; Cancel the pending existence activation before it fires.
(defrule remove-transient-support
  (declare (salience 10))
  ?phase <- (phase transient)
  ?support <- (sym b)
  => (retract ?phase ?support) (assert (phase finished))
  (printout t "transient-removed" crlf))
(defrule complete (declare (salience -100)) => (printout t "done" crlf))
