(deftemplate p (slot v))
(deffacts seed (p (v 1)))
(defrule run ?old <- (p) =>
  (printout t "before" crlf)
  (assert (p (v 2)))
  (reset)
  (printout t "after:" (fact-index ?old) ":" (find-all-facts ((?f p)) TRUE) crlf)
  (halt))
