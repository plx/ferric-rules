(defrule probe =>
(printout t "precision:[" (format nil "%.0c|%.3c|%4.0c|%-4.3c" 65 65 65 65) "]" crlf)
(printout t "empty:[" (format nil "%c|%4c|%-4c:end" "" "" "") "]" crlf)
(printout t "zero:[" (format nil "%c|%4c|%-4c:end" 0 256 0) "]" crlf)
(printout t "bytes:[" (format nil "%c|%3c|%c|%c" "é" "é" -1 255) "]" crlf)
)
