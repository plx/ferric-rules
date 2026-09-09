(deffunction remember (?flag ?value)
  (if ?flag then (bind ?local ?value))
  ?local)
(defrule first-call => (printout t "first:" (remember TRUE 10) crlf))
;; CALL AFTER COMPLETION
(defrule next-call =>
  (printout t (remember FALSE 20) crlf)
  (printout t "after-error" crlf))
