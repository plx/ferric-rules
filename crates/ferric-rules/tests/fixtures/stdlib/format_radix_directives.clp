;; #340 pinned CLIPS characterization: radix-directives

(deffacts startup (go))
(defrule exercise (go) =>
(printout t "[" (format nil "%08o|%08x|%08u" 65 65 65) "]" crlf)
(printout t "[" (format nil "%08.3o|%08.3x|%08.3u" 65 65 65) "]" crlf)
(printout t "[" (format nil "%o|%x|%u" -1 -1 -1) "]" crlf)
)
