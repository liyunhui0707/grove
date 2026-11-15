const buttons = [...document.querySelectorAll('[data-rating]')];
const state = document.querySelector('#rating-state');
function rate(value) {
  const rating = value === 'clear' ? null : Number(value);
  state.textContent = rating === null ? '未评分' : rating === 0 ? '淘汰候选' : rating + ' 星';
  for (const button of buttons) {
    const n = Number(button.dataset.rating);
    const filled = rating !== null && rating > 0 && n > 0 && n <= rating;
    button.classList.toggle('filled', filled);
    if (button.dataset.rating !== 'clear') {
      button.setAttribute('aria-pressed', String(n === rating));
      button.firstChild.textContent = n === 0 ? '⊘' : filled ? '★' : '☆';
    }
  }
}
for (const button of buttons) button.addEventListener('click', () => rate(button.dataset.rating));
