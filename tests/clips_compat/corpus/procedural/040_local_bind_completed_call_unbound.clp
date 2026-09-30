;; A local bound by a completed call is unbound in the next call.
(deffunction remember (?flag ?value)
  (if ?flag then (bind ?local ?value))
  ?local)
(defrule first-call (declare (salience 10))
  => (printout t "first:" (remember TRUE 10) crlf))
(defrule next-call =>
  (printout t (remember FALSE 20) crlf)
  (printout t "after-error" crlf))
