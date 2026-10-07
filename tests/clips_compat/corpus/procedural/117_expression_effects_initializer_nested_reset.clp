(deffunction restart () (reset) 7)
(deffacts seed (p before (reset) after) (q (restart)))
(defrule run (p $?fields) (q ?value) =>
  (printout t ?fields ":" ?value crlf))
