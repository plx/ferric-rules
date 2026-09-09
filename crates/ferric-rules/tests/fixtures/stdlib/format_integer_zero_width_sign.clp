;; #340 pinned CLIPS characterization: integer-zero-width-sign

(deffacts startup (go))
(defrule exercise (go) =>
(printout t "[" (format nil "%04d" 7) "]" crlf)
(printout t "[" (format nil "%04d" -7) "]" crlf)
(printout t "[" (format nil "%04d" 0) "]" crlf)
(printout t "[" (format nil "%04d" 12345) "]" crlf)
(printout t "[" (format nil "%04d" -12345) "]" crlf)
(printout t "[" (format nil "%01d|%00d|%d" 7 7 7) "]" crlf)
)
