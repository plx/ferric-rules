;; #340 pinned CLIPS characterization: float-zero-and-precision

(deffacts startup (go))
(defrule exercise (go) =>
(printout t "[" (format nil "%08.2f" 2.5) "]" crlf)
(printout t "[" (format nil "%08.2f" -2.5) "]" crlf)
(printout t "[" (format nil "%-08.2f" 2.5) "]" crlf)
(printout t "[" (format nil "%-08.2f" -2.5) "]" crlf)
(printout t "[" (format nil "%012.2e" 2.5) "]" crlf)
(printout t "[" (format nil "%012.2e" -2.5) "]" crlf)
(printout t "[" (format nil "%010.3g" 2.5) "]" crlf)
(printout t "[" (format nil "%010.3g" -2.5) "]" crlf)
)
