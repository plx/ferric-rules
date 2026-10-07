(deffunction mark (?tag ?value) (printout t ?tag) ?value)
(defrule run =>
 (printout t (+ (mark A 1) (expand$ (mark B (create$ 2 3)))
   (mark C 4) (expand$ (mark D (create$ 5)))) crlf)
 (printout t (and FALSE (mark skipped TRUE) (expand$ (mark E (create$ TRUE)))) crlf))
