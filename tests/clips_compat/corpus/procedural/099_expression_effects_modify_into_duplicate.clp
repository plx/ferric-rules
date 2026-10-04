(deftemplate p (slot v))
(defrule run =>
  (bind ?a (assert (p (v 1))))
  (bind ?b (assert (p (v 2))))
  (printout t "result:[" (modify ?a (v 2)) "] old:" (fact-existp ?a) " existing:" (fact-existp ?b) crlf)
  (printout t (find-all-facts ((?f p)) TRUE) crlf))
