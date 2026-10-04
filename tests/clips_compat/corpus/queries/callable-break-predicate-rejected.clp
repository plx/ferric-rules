(deftemplate item (slot n))
(defrule bad =>
  (while TRUE do (do-for-all-facts ((?f item)) (break) (printout t unexpected crlf))))
