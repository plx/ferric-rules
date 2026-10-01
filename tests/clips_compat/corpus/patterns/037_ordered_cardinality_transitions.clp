; Issue #320: wrong-width facts must not support exists or block not/NCC.
(deffacts input (phase initial) (row a b) (marker))
(defrule absent
  ?p <- (phase initial)
  (not (row ?))
  (not (and (row ?) (marker)))
  =>
  (printout t "absent" crlf)
  (retract ?p)
  (assert (phase present) (row a)))
(defrule present
  ?p <- (phase present)
  ?r <- (row ?)
  (exists (row ?))
  =>
  (printout t "present" crlf)
  (retract ?p ?r)
  (assert (phase removed)))
(defrule removed
  (phase removed)
  (not (row ?))
  (not (and (row ?) (marker)))
  => (printout t "removed" crlf))
