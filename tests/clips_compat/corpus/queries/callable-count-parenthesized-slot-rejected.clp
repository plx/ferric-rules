(deftemplate item (slot end))
(defrule bad =>
  (do-for-all-facts ((?f item)) TRUE
    (loop-for-count (?f:end) do (printout t unexpected crlf))))
